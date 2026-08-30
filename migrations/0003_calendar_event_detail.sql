-- Richer calendar events, now that real sources (ICS feeds, RSS, scraped pages)
-- replace the stub. The universal "spine" of any event gets typed columns so the
-- calendar can query/sort/dedup on them; source-specific extras are intentionally
-- left out (add them per-source later if a real query needs them).
ALTER TABLE calendar_events
    ADD COLUMN uid         TEXT,        -- stable id from the source (ICS UID, etc.)
    ADD COLUMN ends_at     TIMESTAMPTZ,
    ADD COLUMN url         TEXT,
    ADD COLUMN description TEXT;

-- Dedup strategy, split by whether the source hands us a stable UID:
--
--   * feeds WITH a UID  -> key on (source, uid). Title/time can change on the
--     source side (a talk gets renamed, a start time shifts) and it's still the
--     same event, so keying on UID is what keeps re-runs from duplicating.
--   * feeds WITHOUT a UID (most scrapes) -> fall back to the natural key
--     (source, title, starts_at), same as before.
--
-- Two partial unique indexes, mutually exclusive on uid null-ness, so a row is
-- governed by exactly one of them. This replaces the old blanket UNIQUE.
ALTER TABLE calendar_events
    DROP CONSTRAINT calendar_events_source_title_starts_at_key;

CREATE UNIQUE INDEX calendar_events_uid_key
    ON calendar_events (source, uid) WHERE uid IS NOT NULL;

CREATE UNIQUE INDEX calendar_events_natural_key
    ON calendar_events (source, title, starts_at) WHERE uid IS NULL;
