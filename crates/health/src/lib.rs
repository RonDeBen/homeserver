//! Health job logic, kept out of `main` so it can be driven from the `serve`
//! daemon, `hsctl`, and tests through the same functions — mirrors the calendar
//! crate's shape.
//!
//! Every manual signal follows one persist-then-announce path: a typed trigger
//! event arrives on the bus (from `hsctl health …`, later from the gateway),
//! the handler writes a typed row, then emits a [`HealthObserved`] — the seam
//! the future insights job (and a JetStream stream) reads. Producers announce
//! before any consumer exists, deliberately.
//!
//! Fasting adds one wrinkle: a fast with a `target_hours` arms a durable
//! one-shot timer (via the orchestrator) that fires [`FastTargetReached`] when
//! the target elapses, so the system can ping you. The ping is guarded at fire
//! time on the fast still being open — timers have no cancel path.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use common::{Context, Event};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tracing::{info, warn};

// ---- Trigger/fact events (published by hsctl / the gateway) -----------------

/// A fast just started. `target_hours` is optional; when set, a ping is armed.
#[derive(Debug, Serialize, Deserialize)]
pub struct FastStarted {
    pub started_at: DateTime<Utc>,
    pub target_hours: Option<f64>,
}

impl Event for FastStarted {
    const SUBJECT: &'static str = "health.fast.started";
}

/// A fast just ended (the most recent open one).
#[derive(Debug, Serialize, Deserialize)]
pub struct FastEnded {
    pub ended_at: DateTime<Utc>,
}

impl Event for FastEnded {
    const SUBJECT: &'static str = "health.fast.ended";
}

/// A weigh-in was recorded.
#[derive(Debug, Serialize, Deserialize)]
pub struct WeightRecorded {
    pub measured_at: DateTime<Utc>,
    pub weight_lbs: f64,
    pub note: Option<String>,
}

impl Event for WeightRecorded {
    const SUBJECT: &'static str = "health.weight.recorded";
}

/// One lifting set was recorded.
#[derive(Debug, Serialize, Deserialize)]
pub struct LiftRecorded {
    pub performed_at: DateTime<Utc>,
    pub exercise: String,
    pub weight_lbs: f64,
    pub reps: i32,
    pub note: Option<String>,
}

impl Event for LiftRecorded {
    const SUBJECT: &'static str = "health.lift.recorded";
}

// ---- Announcement + ping events ---------------------------------------------

/// Announcement: a health signal was stored. Downstream (insights, digest)
/// subscribes to react. `domain` is the kind ("fast" | "weight" | "lift"),
/// `detail` a short human summary for logs and prompts.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthObserved {
    pub domain: String,
    pub detail: String,
}

impl Event for HealthObserved {
    const SUBJECT: &'static str = "health.observed";
}

/// A fast's target elapsed. Fired by the orchestrator's durable timer, which
/// the health job armed on `fast start`. Carries the fast's id so the handler
/// can confirm it's still open before pinging.
#[derive(Debug, Serialize, Deserialize)]
pub struct FastTargetReached {
    pub fast_id: i64,
}

impl Event for FastTargetReached {
    const SUBJECT: &'static str = "health.fast.target_reached";
}

// ---- Handlers ---------------------------------------------------------------

/// Start a fast: insert the row, arm the target ping if a target was given,
/// then announce.
pub async fn record_fast_started(ctx: &Context, ev: FastStarted) -> Result<()> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO fasts (started_at, target_hours) VALUES ($1, $2) RETURNING id",
    )
    .bind(ev.started_at)
    .bind(ev.target_hours)
    .fetch_one(&ctx.db)
    .await?;
    info!(fast_id = id, started_at = %ev.started_at, "fast started");

    if let Some(hours) = ev.target_hours {
        // Arm a durable one-shot timer through the orchestrator: it survives a
        // restart and fires `health.fast.target_reached` when the target hits.
        let fire_at = ev.started_at + hours_to_duration(hours);
        let payload = serde_json::to_value(FastTargetReached { fast_id: id })?;
        common::bus::emit(
            &ctx.bus,
            &orchestrator::TimerCreate {
                subject: FastTargetReached::SUBJECT.to_string(),
                payload,
                fire_at,
            },
        )
        .await?;
        info!(fast_id = id, %fire_at, target_hours = hours, "armed fast-target ping");
    }

    announce(ctx, "fast", format!("started fast #{id}")).await
}

/// End the most recent open fast, then announce.
pub async fn record_fast_ended(ctx: &Context, ev: FastEnded) -> Result<()> {
    let result = sqlx::query(
        "UPDATE fasts SET ended_at = $1
         WHERE id = (SELECT id FROM fasts WHERE ended_at IS NULL
                     ORDER BY started_at DESC LIMIT 1)",
    )
    .bind(ev.ended_at)
    .execute(&ctx.db)
    .await?;

    if result.rows_affected() == 0 {
        warn!("fast end requested but no open fast");
        return Ok(());
    }
    info!(ended_at = %ev.ended_at, "fast ended");
    announce(ctx, "fast", "ended fast".to_string()).await
}

/// Store a weigh-in (idempotent on source+time), then announce.
pub async fn record_weight(ctx: &Context, ev: WeightRecorded) -> Result<()> {
    let stored = sqlx::query(
        "INSERT INTO weigh_ins (measured_at, weight_lbs, source, note)
         VALUES ($1, $2, 'manual', $3)
         ON CONFLICT (source, measured_at) DO NOTHING",
    )
    .bind(ev.measured_at)
    .bind(ev.weight_lbs)
    .bind(&ev.note)
    .execute(&ctx.db)
    .await?
    .rows_affected();

    if stored == 0 {
        info!(
            weight_lbs = ev.weight_lbs,
            "weigh-in already recorded, skipped"
        );
        return Ok(());
    }
    info!(weight_lbs = ev.weight_lbs, "weigh-in recorded");
    announce(ctx, "weight", format!("{} lbs", ev.weight_lbs)).await
}

/// Store one lifting set (idempotent on time+exercise), then announce.
pub async fn record_lift(ctx: &Context, ev: LiftRecorded) -> Result<()> {
    let stored = sqlx::query(
        "INSERT INTO lifts (performed_at, exercise, weight_lbs, reps, note)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (performed_at, exercise) DO NOTHING",
    )
    .bind(ev.performed_at)
    .bind(&ev.exercise)
    .bind(ev.weight_lbs)
    .bind(ev.reps)
    .bind(&ev.note)
    .execute(&ctx.db)
    .await?
    .rows_affected();

    if stored == 0 {
        info!(exercise = %ev.exercise, "lift set already recorded, skipped");
        return Ok(());
    }
    info!(exercise = %ev.exercise, weight_lbs = ev.weight_lbs, reps = ev.reps, "lift recorded");
    announce(
        ctx,
        "lift",
        format!("{} {}x{}", ev.exercise, ev.weight_lbs, ev.reps),
    )
    .await
}

/// Fired when a fast's target elapses. Ping only if the fast is still open —
/// an ended-early fast leaves the timer to fire harmlessly (no cancel path).
pub async fn record_fast_target_reached(ctx: &Context, ev: FastTargetReached) -> Result<()> {
    let ended_at: Option<Option<DateTime<Utc>>> =
        sqlx::query_scalar("SELECT ended_at FROM fasts WHERE id = $1")
            .bind(ev.fast_id)
            .fetch_optional(&ctx.db)
            .await?;

    match ended_at {
        Some(None) => {
            // Still open — this is the ping.
            info!(fast_id = ev.fast_id, "fast target reached — ping");
            announce(
                ctx,
                "fast",
                format!("target reached for fast #{}", ev.fast_id),
            )
            .await
        }
        Some(Some(_)) => {
            info!(
                fast_id = ev.fast_id,
                "fast target timer fired but fast already ended, skipping"
            );
            Ok(())
        }
        None => {
            warn!(
                fast_id = ev.fast_id,
                "fast target timer fired for unknown fast"
            );
            Ok(())
        }
    }
}

/// Publish the "a health signal was stored" announcement.
async fn announce(ctx: &Context, domain: &str, detail: String) -> Result<()> {
    common::bus::emit(
        &ctx.bus,
        &HealthObserved {
            domain: domain.to_string(),
            detail,
        },
    )
    .await?;
    info!(
        subject = HealthObserved::SUBJECT,
        domain, "published observation"
    );
    Ok(())
}

// ---- Reads ------------------------------------------------------------------

/// Where a current fast stands relative to its target. Surfaced by the gateway
/// read API (Phase 2); `hsctl` stays bus-only so it doesn't query this.
#[derive(Debug, Serialize)]
pub struct FastStatus {
    pub fast_id: i64,
    pub started_at: DateTime<Utc>,
    pub target_hours: Option<f64>,
    pub elapsed_hours: f64,
    /// "in_window" (target set, not yet met), "target_met" (target set, met),
    /// or "open" (no target).
    pub state: String,
}

/// Status of the current open fast, if any.
pub async fn fast_status(db: &PgPool) -> Result<Option<FastStatus>> {
    let row = sqlx::query(
        "SELECT id, started_at, target_hours FROM fasts
         WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(db)
    .await?;

    let Some(row) = row else { return Ok(None) };
    let fast_id: i64 = row.get("id");
    let started_at: DateTime<Utc> = row.get("started_at");
    let target_hours: Option<f64> = row.get("target_hours");

    let elapsed_hours = (Utc::now() - started_at).num_seconds() as f64 / 3600.0;
    let state = match target_hours {
        Some(t) if elapsed_hours >= t => "target_met",
        Some(_) => "in_window",
        None => "open",
    }
    .to_string();

    Ok(Some(FastStatus {
        fast_id,
        started_at,
        target_hours,
        elapsed_hours,
        state,
    }))
}

/// Hours as a chrono `Duration`, preserving sub-hour precision (milliseconds).
fn hours_to_duration(hours: f64) -> Duration {
    Duration::milliseconds((hours * 3_600_000.0) as i64)
}
