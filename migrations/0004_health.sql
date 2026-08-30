-- Health domain: the first tables of the health hub. Manual signals for now
-- (fasting, weigh-ins, lifting), driven by `hsctl health …`; pushed/polled
-- sources land in the same tables later via the gateway and pollers.
--
-- Typed columns, not jsonb (project convention): the health job and the future
-- insights/gateway readers query and sort on these directly. Money/measurement
-- values are DOUBLE PRECISION rather than NUMERIC so no extra sqlx feature is
-- needed — precision is plenty for one household's tracking.

-- Intermittent fasting. An open fast is `ended_at IS NULL`. `target_hours` is a
-- general duration (named presets like 16:8 are entry-layer sugar that map to
-- hours); when set, the health job arms a durable timer to ping when it elapses.
CREATE TABLE IF NOT EXISTS fasts (
    id           BIGSERIAL PRIMARY KEY,
    started_at   TIMESTAMPTZ      NOT NULL,
    target_hours DOUBLE PRECISION,
    ended_at     TIMESTAMPTZ,
    note         TEXT,
    created_at   TIMESTAMPTZ      NOT NULL DEFAULT now()
);

-- Weigh-ins. `source` distinguishes manual entry from a future smart-scale
-- poller; the natural key keeps a re-sent reading from duplicating.
CREATE TABLE IF NOT EXISTS weigh_ins (
    id         BIGSERIAL PRIMARY KEY,
    measured_at TIMESTAMPTZ      NOT NULL,
    weight_lbs  DOUBLE PRECISION NOT NULL,
    source      TEXT             NOT NULL,
    note        TEXT,
    created_at  TIMESTAMPTZ      NOT NULL DEFAULT now(),
    UNIQUE (source, measured_at)
);

-- Lifting. One row per set. The natural key (a given exercise at an exact
-- instant is one set) makes a re-sent set a no-op; distinct sets differ in time.
CREATE TABLE IF NOT EXISTS lifts (
    id          BIGSERIAL PRIMARY KEY,
    performed_at TIMESTAMPTZ      NOT NULL,
    exercise     TEXT             NOT NULL,
    weight_lbs   DOUBLE PRECISION NOT NULL,
    reps         INT              NOT NULL,
    note         TEXT,
    created_at   TIMESTAMPTZ      NOT NULL DEFAULT now(),
    UNIQUE (performed_at, exercise)
);
