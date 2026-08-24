CREATE TABLE IF NOT EXISTS calendar_events (
    id         BIGSERIAL PRIMARY KEY,
    source     TEXT        NOT NULL,
    title      TEXT        NOT NULL,
    starts_at  TIMESTAMPTZ NOT NULL,
    location   TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source, title, starts_at)
);
