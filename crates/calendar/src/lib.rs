//! Calendar job logic, kept out of `main` so it can be driven from the `serve`
//! daemon, the `run-once` CLI subcommand, `hsctl`, and tests — all through the
//! same functions. Triggering is a thin shell around [`refresh`]/[`submit`].
//!
//! Two triggers converge on one internal model ([`CalendarEvent`]) and one
//! persist-then-announce path: a scheduled [`CalendarRefresh`] (pull from every
//! configured source) and a [`UserSubmittedEvent`] (you, adding an event by
//! hand). Both end in stored rows and a [`CalendarUpdated`] on the bus.
//!
//! Where events come from lives in [`sources`]: an [`EventSource`] trait with a
//! generic ICS and RSS adapter plus per-site HTML scrapers, all selected from
//! `deploy/sources.toml`. `refresh` just loops over them.

pub mod sources;

use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use common::{Context, Event};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};

pub use sources::EventSource;

/// Where the source-of-truth list of calendar sources lives. Overridable via
/// `SOURCES_FILE` so tests / alternate deploys can point elsewhere — mirrors
/// the orchestrator's `SCHEDULES_FILE`.
fn sources_path() -> String {
    std::env::var("SOURCES_FILE").unwrap_or_else(|_| "crates/calendar/sources.toml".to_string())
}

/// Trigger: ask the calendar to do a refresh (pull from every source) pass. The
/// orchestrator publishes this on a schedule; `hsctl calendar refresh` by hand.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CalendarRefresh {}

impl Event for CalendarRefresh {
    const SUBJECT: &'static str = "calendar.refresh";
}

/// Trigger: a calendar event submitted by a person, not scraped.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserSubmittedEvent {
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub location: Option<String>,
}

impl Event for UserSubmittedEvent {
    const SUBJECT: &'static str = "calendar.event.submitted";
}

/// Announcement: the calendar changed. Downstream jobs (e.g. a future
/// `gcal-sync`) subscribe to react.
#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarUpdated {
    pub source: String,
    pub stored: u64,
}

impl Event for CalendarUpdated {
    const SUBJECT: &'static str = "calendar.updated";
}

/// Internal normalized model — what every source produces and what gets stored.
/// Not a bus message; it's the calendar's own representation, the "spine" every
/// source is normalized into. `source` is stamped by [`refresh`] from the
/// source's id, so adapters leave it empty.
#[derive(Debug)]
pub struct CalendarEvent {
    pub source: String,
    /// Stable id from the source (ICS `UID`, feed guid). Drives dedup when
    /// present; `None` falls back to the natural key.
    pub uid: Option<String>,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
}

/// Refresh pass: build every configured source, fetch each, persist, announce.
///
/// The source list is re-read from `sources.toml` each pass, so editing the
/// file takes effect on the next refresh without restarting the daemon. A
/// source that fails to *fetch* is logged and skipped — one dead feed never
/// blocks the rest. (A malformed *config* file, by contrast, fails the pass:
/// that's a misconfiguration to fix, not a transient hiccup.)
pub async fn refresh(ctx: &Context) -> Result<()> {
    let path = sources_path();
    let sources = sources::build_all(&path).with_context(|| format!("loading sources ({path})"))?;
    info!(count = sources.len(), file = %path, "loaded sources");

    let mut total_stored = 0;
    for source in &sources {
        let id = source.id();
        match source.fetch().await {
            Ok(events) => {
                let stored = store_events(&ctx.db, id, &events).await?;
                info!(
                    source = id,
                    fetched = events.len(),
                    stored,
                    "source refreshed"
                );
                total_stored += stored;
            }
            Err(e) => warn!(source = id, error = %e, "source fetch failed, skipping"),
        }
    }
    info!(total_stored, "refresh complete");

    announce(ctx, "calendar", total_stored).await
}

/// Store one hand-submitted event and announce, reusing the same persist path.
pub async fn submit(ctx: &Context, event: UserSubmittedEvent) -> Result<()> {
    let ev = CalendarEvent {
        source: String::new(),
        uid: None,
        title: event.title,
        starts_at: event.starts_at,
        ends_at: None,
        location: event.location,
        url: None,
        description: None,
    };
    let stored = store_events(&ctx.db, "user", std::slice::from_ref(&ev)).await?;
    info!(stored, title = %ev.title, "stored user-submitted event");

    announce(ctx, "user", stored).await
}

/// Publish the "calendar changed" event.
async fn announce(ctx: &Context, source: &str, stored: u64) -> Result<()> {
    common::bus::emit(
        &ctx.bus,
        &CalendarUpdated {
            source: source.into(),
            stored,
        },
    )
    .await?;
    info!(
        subject = CalendarUpdated::SUBJECT,
        source, "published update"
    );
    Ok(())
}

/// Insert events under `source`, skipping ones we already have. Returns how many
/// rows were actually new.
///
/// Dedup mirrors the schema's two partial unique indexes (see migration 0003):
/// events with a `uid` conflict on `(source, uid)`; those without fall back to
/// the natural key `(source, title, starts_at)`. Either way it's `ON CONFLICT
/// DO NOTHING`, so a re-run stores 0 — the "safe to run twice" discipline the
/// scheduled refresh depends on.
async fn store_events(db: &PgPool, source: &str, events: &[CalendarEvent]) -> Result<u64> {
    let mut new_rows = 0;
    for e in events {
        // Only the ON CONFLICT arbiter differs between the two dedup paths; the
        // column list is identical.
        let conflict = if e.uid.is_some() {
            "(source, uid) WHERE uid IS NOT NULL"
        } else {
            "(source, title, starts_at) WHERE uid IS NULL"
        };
        let sql = format!(
            "INSERT INTO calendar_events
                 (source, uid, title, starts_at, ends_at, location, url, description)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT {conflict} DO NOTHING"
        );
        let result = sqlx::query(&sql)
            .bind(source)
            .bind(&e.uid)
            .bind(&e.title)
            .bind(e.starts_at)
            .bind(e.ends_at)
            .bind(&e.location)
            .bind(&e.url)
            .bind(&e.description)
            .execute(db)
            .await?;
        new_rows += result.rows_affected();
    }
    Ok(new_rows)
}
