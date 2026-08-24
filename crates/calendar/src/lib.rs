//! Calendar job logic, kept out of `main` so it can be driven from the `serve`
//! daemon, the `run-once` CLI subcommand, `hsctl`, and tests — all through the
//! same functions. Triggering is a thin shell around [`refresh`]/[`submit`].
//!
//! Two triggers converge on one internal model ([`CalendarEvent`]) and one
//! persist-then-announce path: a scheduled [`CalendarRefresh`] (scrape) and a
//! [`UserSubmittedEvent`] (you, adding an event by hand). Both end in a stored
//! row and a [`CalendarUpdated`] on the bus.
//!
//! `fetch_events` is stubbed for now. Real per-site scrapers will slot in
//! behind it (a future `EventSource` trait, once there are real sources to
//! shape it around) without touching the trigger wiring.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use common::{Context, Event};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

/// Trigger: ask the calendar to do a refresh (scrape) pass. The orchestrator
/// publishes this on a schedule; `hsctl calendar refresh` publishes it by hand.
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

/// Internal normalized model — what every trigger path produces and what gets
/// stored. Not a bus message; it's the calendar's own representation.
#[derive(Debug)]
pub struct CalendarEvent {
    pub source: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub location: Option<String>,
}

/// Scrape pass: fetch from all sources -> persist -> announce.
pub async fn refresh(ctx: &Context) -> Result<()> {
    let events = fetch_events();
    info!(count = events.len(), "fetched events");

    let stored = store_events(&ctx.db, &events).await?;
    info!(stored, "stored new events");

    announce(ctx, "calendar", stored).await
}

/// Store one hand-submitted event and announce, reusing the same persist path.
pub async fn submit(ctx: &Context, event: UserSubmittedEvent) -> Result<()> {
    let ev = CalendarEvent {
        source: "user".into(),
        title: event.title,
        starts_at: event.starts_at,
        location: event.location,
    };
    let stored = store_events(&ctx.db, std::slice::from_ref(&ev)).await?;
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
    info!(subject = CalendarUpdated::SUBJECT, source, "published update");
    Ok(())
}

/// Stubbed event source. Swap for real ICS/scrape logic later.
///
/// Times are anchored to midnight-today (not `now`) so the same event has a
/// stable key across runs — that's what makes the dedup in `store_events`
/// actually hold: run twice, the second pass stores 0.
fn fetch_events() -> Vec<CalendarEvent> {
    let midnight = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc();
    vec![
        CalendarEvent {
            source: "stub".into(),
            title: "Water the tomatoes".into(),
            starts_at: midnight + Duration::hours(18),
            location: Some("garden".into()),
        },
        CalendarEvent {
            source: "stub".into(),
            title: "Farmers market".into(),
            starts_at: midnight + Duration::days(2) + Duration::hours(9),
            location: Some("downtown".into()),
        },
    ]
}

/// Insert events, skipping ones we already have (dedup on source+title+start).
/// Returns how many rows were actually new.
async fn store_events(db: &PgPool, events: &[CalendarEvent]) -> Result<u64> {
    let mut new_rows = 0;
    for e in events {
        let result = sqlx::query(
            "INSERT INTO calendar_events (source, title, starts_at, location)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (source, title, starts_at) DO NOTHING",
        )
        .bind(&e.source)
        .bind(&e.title)
        .bind(e.starts_at)
        .bind(&e.location)
        .execute(db)
        .await?;
        new_rows += result.rows_affected();
    }
    Ok(new_rows)
}
