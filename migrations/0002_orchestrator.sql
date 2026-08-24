-- Recurring, cron-driven triggers. The source of truth is deploy/schedules.toml,
-- synced into this table on orchestrator startup; `next_fire` is the precomputed
-- next occurrence, advanced each time the schedule fires.
CREATE TABLE IF NOT EXISTS schedules (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT        NOT NULL UNIQUE,
    cron       TEXT        NOT NULL,
    subject    TEXT        NOT NULL,
    payload    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    enabled    BOOLEAN     NOT NULL DEFAULT TRUE,
    next_fire  TIMESTAMPTZ,
    last_fired TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Durable one-shot timers/reminders, created at runtime over the bus
-- (`schedule.timer.create`). Fired once when due, then deleted.
CREATE TABLE IF NOT EXISTS timers (
    id         BIGSERIAL PRIMARY KEY,
    subject    TEXT        NOT NULL,
    payload    JSONB       NOT NULL DEFAULT '{}'::jsonb,
    fire_at    TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS timers_fire_at_idx ON timers (fire_at);
