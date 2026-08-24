//! Orchestrator logic: the publisher of time. It turns schedules and timers
//! into bus messages and forgets about them — it never routes results or knows
//! what any subject means. Two kinds of scheduled thing:
//!
//! - **Recurring schedules** — declared in `deploy/schedules.toml` (the source
//!   of truth), reconciled one-way into the `schedules` table on boot
//!   ([`load_schedules`] + [`sync_schedules`]). The table is never hand-edited.
//! - **One-shot timers** — created at runtime over the bus ([`TimerCreate`]),
//!   persisted in `timers`, fired once, deleted. Durable across restarts.
//!
//! [`tick`] fires everything currently due; [`next_sleep`] says how long until
//! the next thing is due. The binary drives the loop.

use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use common::{Context, Event};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

/// The longest we'll sleep between ticks even when nothing is due soon, so a
/// clock jump or an externally-inserted row is never missed by more than this.
const MAX_SLEEP: Duration = Duration::from_secs(60);

/// Bus request to schedule a durable one-shot timer. Published by any job (or
/// `hsctl timer`); the orchestrator persists it and fires `subject`+`payload`
/// when `fire_at` arrives.
#[derive(Debug, Serialize, Deserialize)]
pub struct TimerCreate {
    pub subject: String,
    #[serde(default = "empty_object")]
    pub payload: serde_json::Value,
    pub fire_at: DateTime<Utc>,
}

impl Event for TimerCreate {
    const SUBJECT: &'static str = "schedule.timer.create";
}

/// One `[[schedule]]` entry from `schedules.toml`.
#[derive(Debug, Deserialize)]
pub struct ScheduleSpec {
    pub name: String,
    pub cron: String,
    pub subject: String,
    #[serde(default = "empty_object")]
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SchedulesFile {
    #[serde(default)]
    schedule: Vec<ScheduleSpec>,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

/// Parse `schedules.toml` into schedule specs.
pub fn load_schedules(path: &str) -> Result<Vec<ScheduleSpec>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading schedules file {path}"))?;
    let file: SchedulesFile =
        toml::from_str(&text).with_context(|| format!("parsing schedules file {path}"))?;
    Ok(file.schedule)
}

/// Reconcile the `schedules` table to match `specs` (the file is authoritative):
/// upsert every spec with a freshly computed `next_fire`, then delete any table
/// row whose name is no longer in the file.
pub async fn sync_schedules(db: &PgPool, specs: &[ScheduleSpec]) -> Result<()> {
    let now = Utc::now();
    for spec in specs {
        let next = next_fire(&spec.cron, now)?;
        sqlx::query(
            "INSERT INTO schedules (name, cron, subject, payload, next_fire, enabled)
             VALUES ($1, $2, $3, $4, $5, TRUE)
             ON CONFLICT (name) DO UPDATE SET
                 cron = EXCLUDED.cron,
                 subject = EXCLUDED.subject,
                 payload = EXCLUDED.payload,
                 next_fire = EXCLUDED.next_fire,
                 enabled = TRUE",
        )
        .bind(&spec.name)
        .bind(&spec.cron)
        .bind(&spec.subject)
        .bind(&spec.payload)
        .bind(next)
        .execute(db)
        .await?;
        info!(name = %spec.name, cron = %spec.cron, subject = %spec.subject, %next, "schedule synced");
    }

    let names: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
    let pruned = sqlx::query("DELETE FROM schedules WHERE name <> ALL($1)")
        .bind(&names)
        .execute(db)
        .await?;
    if pruned.rows_affected() > 0 {
        info!(count = pruned.rows_affected(), "pruned schedules not in file");
    }
    Ok(())
}

/// Persist a one-shot timer.
pub async fn insert_timer(db: &PgPool, timer: &TimerCreate) -> Result<()> {
    sqlx::query("INSERT INTO timers (subject, payload, fire_at) VALUES ($1, $2, $3)")
        .bind(&timer.subject)
        .bind(&timer.payload)
        .bind(timer.fire_at)
        .execute(db)
        .await?;
    Ok(())
}

/// Fire everything currently due: publish each due schedule and advance its
/// `next_fire`; publish each due timer and delete it. Returns how many of each
/// fired.
pub async fn tick(ctx: &Context) -> Result<(u64, u64)> {
    let now = Utc::now();

    let due_schedules =
        sqlx::query("SELECT id, cron, subject, payload FROM schedules WHERE enabled AND next_fire <= $1")
            .bind(now)
            .fetch_all(&ctx.db)
            .await?;
    let mut fired_schedules = 0;
    for row in &due_schedules {
        let id: i64 = row.get("id");
        let cron: String = row.get("cron");
        let subject: String = row.get("subject");
        let payload: serde_json::Value = row.get("payload");

        common::bus::publish_json(&ctx.bus, &subject, &payload).await?;
        let next = next_fire(&cron, now)?;
        sqlx::query("UPDATE schedules SET next_fire = $1, last_fired = $2 WHERE id = $3")
            .bind(next)
            .bind(now)
            .bind(id)
            .execute(&ctx.db)
            .await?;
        info!(%subject, %next, "fired schedule");
        fired_schedules += 1;
    }

    let due_timers = sqlx::query("SELECT id, subject, payload FROM timers WHERE fire_at <= $1")
        .bind(now)
        .fetch_all(&ctx.db)
        .await?;
    let mut fired_timers = 0;
    for row in &due_timers {
        let id: i64 = row.get("id");
        let subject: String = row.get("subject");
        let payload: serde_json::Value = row.get("payload");

        common::bus::publish_json(&ctx.bus, &subject, &payload).await?;
        sqlx::query("DELETE FROM timers WHERE id = $1")
            .bind(id)
            .execute(&ctx.db)
            .await?;
        info!(%subject, "fired timer");
        fired_timers += 1;
    }

    Ok((fired_schedules, fired_timers))
}

/// How long until the next scheduled thing is due, capped at [`MAX_SLEEP`].
/// Zero if something is already due; the cap if nothing is scheduled.
pub async fn next_sleep(db: &PgPool) -> Result<Duration> {
    let now = Utc::now();
    let next_schedule: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT min(next_fire) FROM schedules WHERE enabled")
            .fetch_one(db)
            .await?;
    let next_timer: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT min(fire_at) FROM timers")
            .fetch_one(db)
            .await?;

    let soonest = [next_schedule, next_timer].into_iter().flatten().min();
    let dur = match soonest {
        // `to_std` errors if the target is in the past -> already due -> ZERO.
        Some(at) => (at - now).to_std().unwrap_or(Duration::ZERO),
        None => MAX_SLEEP,
    };
    Ok(dur.min(MAX_SLEEP))
}

/// Next occurrence of `cron` strictly after `after`.
pub fn next_fire(cron: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let schedule = Schedule::from_str(cron).with_context(|| format!("invalid cron: {cron:?}"))?;
    schedule
        .after(&after)
        .next()
        .with_context(|| format!("cron {cron:?} has no upcoming occurrence"))
}
