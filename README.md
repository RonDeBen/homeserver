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
 (daemon) (sensors) (mic→LLM→bass)(batch)(agent) Assistant
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
  bus when due. Recurring runs come from `deploy/schedules.toml` (reconciled
  into Postgres on boot); one-shot timers/reminders are created at runtime over
  the bus (`schedule.timer.create`).

---

## Layout

```
crates/
  common/       shared plumbing: config, DB pool, NATS client, Event trait, router
  calendar/     first job — daemon (serve) reacting to calendar.* messages
  orchestrator/ publisher of time — schedules + timers -> bus
  hsctl/        CLI to inject messages onto the bus (testing / poking by hand)
migrations/         shared SQL migrations (sqlx)
deploy/nats/        NATS server config (JetStream + MQTT enabled)
deploy/schedules.toml   recurring schedules, synced into Postgres by orchestrator
crates/calendar/sources.toml   calendar sources (ICS/RSS/scrape/API), read each refresh
docker-compose.yml      full stack: infra (NATS + Postgres) + daemons
Dockerfile              one image, all binaries (compose sets each service's command)
```

Everything runs in Docker Compose. For a fast dev loop you can still run any job
on the host with `cargo run` against the containerized infra (see below).

---

## Run it

Bring up the whole stack — infra plus the daemons — in one command:

```bash
# 1. Build images and start everything (NATS, Postgres, orchestrator, calendar)
docker compose up -d --build

# 2. Watch it work
docker compose logs -f orchestrator calendar

# 3. Poke it by hand with hsctl (runs in the orchestrator container, on the
#    compose network). See `hsctl --help` for all subcommands.
docker compose exec orchestrator hsctl calendar refresh           # trigger a scrape now
docker compose exec orchestrator hsctl calendar submit "Dentist" 2026-09-01T09:00:00Z
docker compose exec orchestrator hsctl timer 10s calendar.refresh # fire in 10s

# 4. See what got stored
docker compose exec postgres \
  psql -U homeserver -c 'select source, title, starts_at, location from calendar_events;'
```

Recurring schedules live in `deploy/schedules.toml` (mounted into the
orchestrator). Edit it, then `docker compose restart orchestrator` to re-sync.

Calendar sources live in `crates/calendar/sources.toml` (mounted into the
calendar). It's re-read on every refresh, so an edit takes effect on the next
`calendar.refresh` — no restart. Add an ICS/RSS feed with a `[[source]]` entry;
add a scraped site by copying `crates/calendar/src/sources/bears.rs`.

### Dev loop (running a job on the host)

Faster iteration than rebuilding an image: run infra in Docker, the job under
`cargo`.

```bash
docker compose up -d nats postgres   # just infra
cp .env.example .env                  # required — jobs fail fast without it

cargo run -p calendar -- serve        # daemon; reacts to calendar.refresh
cargo run -p calendar -- run-once     # ...or a single pass, then exit
cargo run -p hsctl -- calendar refresh  # inject from the host

# (optional) watch the raw bus — needs the `nats` CLI (github.com/nats-io/natscli)
nats sub 'calendar.>'
```

For fast gateway UI iteration, use the helper script. It starts only Postgres
and NATS in Docker and runs the gateway locally under `cargo-watch`; edits to
gateway/common Rust code or gateway assets rebuild and restart the gateway.

```bash
cargo install cargo-watch   # one-time
scripts/gateway-dev.sh start
scripts/gateway-dev.sh logs
scripts/gateway-dev.sh stop
```

The watcher uses the host values from `.env` when present, with defaults of
Postgres `localhost:5433`, NATS `localhost:4222`, and trusted gateway auth.

> Postgres is on host port **5433** (5432 is often taken by another local
> Postgres); `.env.example` already points there. Inside the compose network,
> jobs reach Postgres at `postgres:5432` and NATS at `nats:4222` — those values
> are set per-service in `docker-compose.yml`, so containers need no `.env`.

---

## Conventions

- **Bus subjects** are dotted. Events are past-tense: `calendar.updated`,
  `garden.moisture.low`, `voice.intent.detected`. Request/reply subjects are
  imperative: `calendar.query`, `media.play`.
- **Migrations** live in `/migrations`, run by whichever job owns the schema via
  `sqlx::migrate!` on startup — idempotent, safe every run.
- **A new job** = a new crate under `crates/`, added to the workspace `members`.
  Copy `calendar` as the template: `lib.rs` holds the logic + its `Event` types
  (payload bound to subject, so `grep "impl.*Event for"` is the message
  catalog); `main.rs` is a thin shell — `common::bus::Subscriptions::new(ctx)`
  with an `.on(handler)` per message it reacts to, then `.serve()`. Trigger
  those messages from the orchestrator (`schedules.toml`) or `hsctl`.
- **Dedup / idempotency** matters because jobs get re-run on a schedule. The
  calendar keys events on `(source, uid)` when a feed gives a stable id, else
  `(source, title, starts_at)`, with `ON CONFLICT DO NOTHING`; follow the same
  "safe to run twice" discipline everywhere.

---

## Planned jobs

Rough shapes, so it's clear what plugs in where:

| Job | Shape | Notes |
|-----|-------|-------|
| **calendar** | daemon | ✅ `serve` reacts to `calendar.refresh` / `calendar.event.submitted`. ✅ real sources via an `EventSource` trait — generic ICS + RSS adapters, per-site HTML scrapers, geo API adapters, all listed in `crates/calendar/sources.toml`. Next: attendance capture feeding the ranker. |
| **orchestrator** | daemon | ✅ schedules (from `schedules.toml`) + durable one-shot timers → bus. |
| **ranker** | batch / agent | LLM scores upcoming calendar events 0–10 against an interest profile + attendance history — "ingest wholesale, rank later." Reacts to `calendar.updated`; publishes `calendar.ranked`. |
| **digest** | scheduled batch | The part you consume: weekly/monthly email of top-ranked upcoming events. Orchestrator fires `digest.weekly` / `digest.monthly`; ships naive-ranked first, LLM behind it. |
| **garden** | sensors + batch | Moisture sensors (MQTT → bus), weather-forecast polling, plantings table. Publishes `garden.moisture.low` etc. |
| **voice-io** | daemon (Python) | mic → wake-word → STT (Whisper) → LLM → TTS → Billy Bass. Holds session state; keep the tight audio loop in-process, publish *intents/results* to the bus for logging + downstream reactions. |
| **media** | batch | Classify / sort / dedup a large video+music library. IO/CPU heavy, cron-driven; ffmpeg from Rust. |
| **health** | agent + batch | Recipes + health logs; an agent that periodically quizzes "how are you feeling / what did you eat," writes to Postgres. |
| **lighting** | policy | Circadian shift + motion-triggered (dimmed late) lights. Let Home Assistant drive the bulbs/Zigbee; this job holds the *policy* and talks to HA over MQTT. |

Billy-Bass-as-whimsical-Alexa spans voice-io (Q&A), media (`media.play`), and
orchestrator (timers) — a good stress test of the whole design.

---

## Roadmap

1. ✅ **Orchestrator crate** — schedules (`schedules.toml`) + durable one-shot
   timers → bus. Calendar-on-a-schedule and the bass's timers from one place.
2. ✅ **Real calendar sources** — an `EventSource` trait behind `refresh`:
   generic `ical`/`rss` adapters (config-only, any URL), per-site HTML scrapers
   (`bears`), and geo API adapters (`ticketmaster`), all selected from
   `crates/calendar/sources.toml`. Adding a feed is a TOML edit; adding a scrape
   is one small `sources/<site>.rs` (copy `sources/bears.rs`). One source failing
   to fetch is logged and skipped — it never aborts the pass.
3. **Event ranking + consumption** — the "so what do I actually go to?" layer on
   top of the calendar. The reason ingestion deliberately *doesn't* over-filter
   ("ingest wholesale, rank later") is so this layer can make the semantic calls
   a rule can't — telling "Adult Beginners Computer Class" from an all-ages
   coding jam. Three jobs, each landing on an existing seam:
   - **ranker** — reacts to `calendar.updated` (and/or a schedule); a cheap LLM
     classification pass scores each upcoming event 0–10 against an interest
     profile + attendance history, writing the score back (a `score` column or a
     `rankings` table). Publishes `calendar.ranked`.
   - **digest** — the consumable end. The orchestrator already owns
     fire-onto-the-bus timing, so weekly/monthly email is a `schedules.toml`
     edit (`digest.weekly` / `digest.monthly`) plus a job that queries the
     top-ranked upcoming events and sends mail. Ship it first with naive ranking
     (soonest first) so the loop closes before the LLM lands, then slot the
     ranker in behind it.
   - **attendance** — the feedback signal that sharpens the ranker: record what
     you actually went to (`hsctl calendar attended …` to start; a periodic
     "what'd you go to?" prompt later, à la the health agent). It's the whole
     training signal — few-shot ground truth, no ML infra.
   Build order: digest (naive) → ranker → attendance, each independently useful.
4. **JetStream streams** — turn on a durable, replayable event log; move the KV
   store into use for "current state" (light states, active timers).
5. **First sensor job (garden)** — proves the MQTT-device → NATS-service path.
6. **voice-io** — the first Python participant; validates the polyglot seam.

---

## Open questions / decisions to revisit

- Whether Home Assistant is the device-driver layer for *all* smart-home, or
  just lighting.
- Where the LLM runs (local model vs. hosted API) — for the voice path (latency
  matters) and for the calendar ranker (cheap batch classification, latency
  doesn't). Could be different answers per job.
- Auth on the bus — currently none (single trusted LAN). Revisit if anything
  becomes internet-reachable.
- Backups for Postgres + the JetStream store dir.
