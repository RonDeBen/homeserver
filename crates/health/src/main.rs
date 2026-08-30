//! Health job — manual health signals (fasting, weigh-ins, lifting), following
//! the calendar template. The work lives in `lib.rs`; this binary is the
//! triggering shell.
//!
//!   serve   long-lived daemon. The `Subscriptions` block below is the whole
//!           story: which messages it listens for and what each triggers.
//!
//! Triggers come from `hsctl health …` (and, later, the gateway). There's no
//! scrape/refresh, so no `run-once` mode.

use anyhow::Result;
use clap::{Parser, Subcommand};
use common::bus::Subscriptions;
use common::Context;
use health::{FastEnded, FastStarted, FastTargetReached, LiftRecorded, WeightRecorded};
use std::sync::Arc;

#[derive(Parser)]
#[command(about = "Health job")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run as a daemon, reacting to health bus messages.
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = common::init().await?;

    // Idempotent, so safe on every startup.
    common::db::migrate(&ctx.db).await?;

    match cli.command {
        Command::Serve => {
            // subject -> handler. Awaits live in the handler fns, not here.
            Subscriptions::new(ctx)
                .on(on_fast_started)
                .on(on_fast_ended)
                .on(on_weight)
                .on(on_lift)
                .on(on_fast_target_reached)
                .serve()
                .await
        }
    }
}

/// `health.fast.started` -> record the fast, arm the target ping.
async fn on_fast_started(ctx: Arc<Context>, ev: FastStarted) -> Result<()> {
    health::record_fast_started(&ctx, ev).await
}

/// `health.fast.ended` -> close the open fast.
async fn on_fast_ended(ctx: Arc<Context>, ev: FastEnded) -> Result<()> {
    health::record_fast_ended(&ctx, ev).await
}

/// `health.weight.recorded` -> store a weigh-in.
async fn on_weight(ctx: Arc<Context>, ev: WeightRecorded) -> Result<()> {
    health::record_weight(&ctx, ev).await
}

/// `health.lift.recorded` -> store a lifting set.
async fn on_lift(ctx: Arc<Context>, ev: LiftRecorded) -> Result<()> {
    health::record_lift(&ctx, ev).await
}

/// `health.fast.target_reached` -> ping if the fast is still open.
async fn on_fast_target_reached(ctx: Arc<Context>, ev: FastTargetReached) -> Result<()> {
    health::record_fast_target_reached(&ctx, ev).await
}
