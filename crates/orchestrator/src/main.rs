//! Orchestrator daemon — the one component that owns time. It reconciles
//! `schedules.toml` into the DB, then runs two things concurrently until
//! shutdown:
//!
//!   - a tick loop that fires everything due onto the bus, sleeping until the
//!     next thing is due (or `MAX_SLEEP`);
//!   - a subscription to `schedule.timer.create` that persists new one-shot
//!     timers and nudges the loop awake so short timers fire promptly.
//!
//! It's structurally different from a normal job (it has a timing loop, not
//! just a `Subscriptions` router), which is why it wires the bus by hand.

use anyhow::Result;
use common::Event;
use futures::StreamExt;
use orchestrator::TimerCreate;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{error, info};

/// Where the recurring-schedule source of truth lives. Overridable so tests /
/// alternate deploys can point elsewhere.
fn schedules_path() -> String {
    std::env::var("SCHEDULES_FILE").unwrap_or_else(|_| "deploy/schedules.toml".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = common::init().await?;
    common::db::migrate(&ctx.db).await?;

    let path = schedules_path();
    let specs = orchestrator::load_schedules(&path)?;
    orchestrator::sync_schedules(&ctx.db, &specs).await?;
    info!(count = specs.len(), file = %path, "schedules loaded");

    let ctx = Arc::new(ctx);
    let wake = Arc::new(Notify::new());

    // Timer intake runs alongside the tick loop; it only writes rows + nudges.
    let timer_task = tokio::spawn(timer_intake(Arc::clone(&ctx), Arc::clone(&wake)));

    run_loop(Arc::clone(&ctx), wake).await?;

    // Loop returned => shutdown signalled. Drop the intake task with it.
    timer_task.abort();
    Ok(())
}

/// Fire due work, then sleep until the next thing is due — but wake early if a
/// new timer arrives (`wake`) or a shutdown signal fires.
async fn run_loop(ctx: Arc<common::Context>, wake: Arc<Notify>) -> Result<()> {
    let shutdown = common::shutdown();
    tokio::pin!(shutdown);

    loop {
        match orchestrator::tick(&ctx).await {
            Ok((s, t)) if s + t > 0 => info!(schedules = s, timers = t, "fired"),
            Ok(_) => {}
            // A bad row shouldn't kill the loop; log and carry on to the sleep.
            Err(e) => error!(error = %e, "tick failed"),
        }

        let nap = orchestrator::next_sleep(&ctx.db).await?;

        tokio::select! {
            _ = &mut shutdown => {
                info!("shutdown signal received, stopping");
                break;
            }
            _ = tokio::time::sleep(nap) => {}
            _ = wake.notified() => {} // new timer: re-tick immediately
        }
    }
    Ok(())
}

/// Subscribe to `schedule.timer.create`, persist each timer, nudge the loop.
async fn timer_intake(ctx: Arc<common::Context>, wake: Arc<Notify>) -> Result<()> {
    let mut sub = ctx.bus.subscribe(TimerCreate::SUBJECT.to_string()).await?;
    info!(subject = TimerCreate::SUBJECT, "subscribed (timer intake)");

    while let Some(msg) = sub.next().await {
        match serde_json::from_slice::<TimerCreate>(&msg.payload) {
            Ok(timer) => match orchestrator::insert_timer(&ctx.db, &timer).await {
                Ok(()) => {
                    info!(subject = %timer.subject, fire_at = %timer.fire_at, "timer scheduled");
                    wake.notify_one();
                }
                Err(e) => error!(error = %e, "failed to persist timer"),
            },
            Err(e) => error!(error = %e, "invalid timer.create payload"),
        }
    }
    Ok(())
}
