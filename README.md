# homeserver

A single project that hosts several independent **jobs** — calendar, gardening,
a voice assistant, media tooling, health tracking, smart-home policy — running
on one old box, coordinated over a message bus. The point is one centralized
codebase where adding a new capability is "write another job and have it listen
on the bus," not "stand up another service."

This runs at home, for one household. It is deliberately **not** built for
scale, multi-tenancy, or the cloud — those constraints (and their complexity)
don't apply here.

---

## Architecture

Three pieces:

1. **A NATS message bus** — the backbone. Jobs don't call each other directly;
   they publish events and subscribe to what they care about. This is what
   makes the system easy to extend (a new job just starts listening) and easy
   to reason about (all cross-job traffic is on the bus).
2. **A shared Postgres** — durable state: calendar events, plantings, recipes,
   health logs, the media catalog. One database, a schema/table set per domain.
3. **Jobs** — independent processes. Rust for systems glue; other languages
   where they're the obvious fit (Python for the Whisper/LLM voice path). Each
   is a bus participant, so language is a per-job choice, not a global one.

```
        orchestrator (scheduled runs, timers, reminders)
                        │  fires when due
   ┌──────────────── NATS bus ────────────────┐
   │        │          │           │       │    │
 calendar  garden   voice-io     media  health  Home
 (batch)  (sensors) (mic→LLM→bass)(batch)(agent) Assistant
   └──────────────► Postgres ◄──────────────┘
```

### Why these choices

- **Message bus over direct calls / in-process actors.** A bus gives
  actor-style message passing *across* processes and languages, plus fault
  isolation (media dedup crashing can't take down the voice assistant). An
  in-process actor system would be elegant but would force everything into one
  runtime — bad, because the AI path wants Python.
- **NATS over Mosquitto/MQTT.** NATS is a service mesh that *also* does
  telemetry: first-class request/reply (voice service can *ask* calendar "what's
  next?"), queue groups, and JetStream (durable replayable streams + a built-in
  KV store, so no separate Redis). It also has a built-in **MQTT interface**, so
  IoT devices that only speak MQTT connect to the *same broker* on 1883 — we
  never have to choose or bridge. See `deploy/nats/nats-server.conf`.
- **Monorepo, Cargo workspace.** Shared plumbing lives once (`crates/common`);
  every job pins the same tokio/sqlx/nats versions via `[workspace.dependencies]`.
- **Orchestrator owns "future intentions."** Scheduled job runs, the bass's
  timers, reminders — all "deliver this later," which neither NATS nor MQTT does
  well. So one component holds them durably in Postgres and fires them onto the
  bus when due. (Not built yet — first planned crate.)

---

## Layout

```
crates/
  common/     shared plumbing: config, DB pool, NATS client, logging
  calendar/   first job — batch: fetch -> persist -> announce
migrations/   shared SQL migrations (sqlx)
deploy/nats/  NATS server config (JetStream + MQTT enabled)
docker-compose.yml   infra only (NATS + Postgres)
```

Infra runs in Docker; jobs run on the host via `cargo` for a fast dev loop.
Long-running daemons (the voice service) will get their own compose services
once they exist — batch jobs don't need to.

---

## Run it

```bash
# 1. Start infra (NATS + Postgres)
docker compose up -d

# 2. Config for local dev (required — jobs fail fast without it)
cp .env.example .env

# 3. Run the calendar job (daemon; reacts to calendar.refresh)
cargo run -p calendar -- serve
#    ...or do a single pass and exit:
cargo run -p calendar -- run-once

# 4. (optional) watch the bus — needs the `nats` CLI
#    https://github.com/nats-io/natscli
nats sub 'calendar.updated'          # run before step 3, in another terminal

# 5. See what it stored
docker compose exec postgres \
  psql -U homeserver -c 'select title, starts_at, location from calendar_events;'
```

> Postgres is on host port **5433** (5432 is often taken by another local
> Postgres). The defaults in `crates/common/src/config.rs` and `.env.example`
> already reflect that, so `cargo run` needs no setup.

---

## Conventions

- **Bus subjects** are dotted. Events are past-tense: `calendar.updated`,
  `garden.moisture.low`, `voice.intent.detected`. Request/reply subjects are
  imperative: `calendar.query`, `media.play`.
- **Migrations** live in `/migrations`, run by whichever job owns the schema via
  `sqlx::migrate!` on startup — idempotent, safe every run.
- **A new job** = a new crate under `crates/`, added to the workspace `members`,
  starting from `let ctx = common::init().await?;`. Copy `calendar` as the
  template.
- **Dedup / idempotency** matters because jobs get re-run on a schedule. The
  calendar keys events on `(source, title, starts_at)` and `ON CONFLICT DO
  NOTHING`; follow the same "safe to run twice" discipline everywhere.

---

## Planned jobs

Rough shapes, so it's clear what plugs in where:

| Job | Shape | Notes |
|-----|-------|-------|
| **calendar** | batch | ✅ skeleton done. Next: real ICS feeds / event-page scrapes behind `fetch_events`. |
| **orchestrator** | daemon | Owns schedules, timers, reminders in Postgres; fires triggers onto the bus. The next thing to build. |
| **garden** | sensors + batch | Moisture sensors (MQTT → bus), weather-forecast polling, plantings table. Publishes `garden.moisture.low` etc. |
| **voice-io** | daemon (Python) | mic → wake-word → STT (Whisper) → LLM → TTS → Billy Bass. Holds session state; keep the tight audio loop in-process, publish *intents/results* to the bus for logging + downstream reactions. |
| **media** | batch | Classify / sort / dedup a large video+music library. IO/CPU heavy, cron-driven; ffmpeg from Rust. |
| **health** | agent + batch | Recipes + health logs; an agent that periodically quizzes "how are you feeling / what did you eat," writes to Postgres. |
| **lighting** | policy | Circadian shift + motion-triggered (dimmed late) lights. Let Home Assistant drive the bulbs/Zigbee; this job holds the *policy* and talks to HA over MQTT. |

Billy-Bass-as-whimsical-Alexa spans voice-io (Q&A), media (`media.play`), and
orchestrator (timers) — a good stress test of the whole design.

---

## Roadmap

1. **Orchestrator crate** — schedules + timers + reminders → bus. Unlocks
   calendar-on-a-schedule and the bass's timers from one place.
2. **Real calendar sources** — swap the stubbed `fetch_events` for ICS/scrape.
3. **JetStream streams** — turn on a durable, replayable event log; move the KV
   store into use for "current state" (light states, active timers).
4. **First sensor job (garden)** — proves the MQTT-device → NATS-service path.
5. **voice-io** — the first Python participant; validates the polyglot seam.

---

## Open questions / decisions to revisit

- Whether Home Assistant is the device-driver layer for *all* smart-home, or
  just lighting.
- Where the LLM runs (local model vs. hosted API) for the voice path.
- Auth on the bus — currently none (single trusted LAN). Revisit if anything
  becomes internet-reachable.
- Backups for Postgres + the JetStream store dir.
