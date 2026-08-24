//! Calendar job — the first job, and the template for the rest.
//!
//! The work lives in `lib.rs`. This binary is the triggering shell:
//!
//!   serve      long-lived daemon. The `Subscriptions` block below is the
//!              whole story: which messages it listens for and what each
//!              triggers. This is the normal mode.
//!   run-once   one refresh pass, then exit — for manual testing
//!              (`cargo run -p calendar -- run-once`) or a cron/systemd model.

use anyhow::Result;
use calendar::{CalendarRefresh, UserSubmittedEvent};
use clap::{Parser, Subcommand};
use common::bus::Subscriptions;
use common::Context;
use std::sync::Arc;

#[derive(Parser)]
#[command(about = "Calendar job")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run as a daemon, reacting to calendar bus messages.
    Serve,
    /// Do a single refresh pass and exit.
    RunOnce,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = common::init().await?;

    // Idempotent, so safe on every startup regardless of mode.
    common::db::migrate(&ctx.db).await?;

    match cli.command {
        Command::RunOnce => calendar::refresh(&ctx).await,
        Command::Serve => {
            // The whole story of the daemon: subject -> handler. The handlers
            // are the named async fns below; awaits live there, not here.
            Subscriptions::new(ctx)
                .on(on_refresh)
                .on(on_submit)
                .serve()
                .await
        }
    }
}

/// `calendar.refresh` -> run a scrape/refresh pass.
async fn on_refresh(ctx: Arc<Context>, _: CalendarRefresh) -> Result<()> {
    calendar::refresh(&ctx).await
}

/// `calendar.event.submitted` -> store a hand-entered event.
async fn on_submit(ctx: Arc<Context>, event: UserSubmittedEvent) -> Result<()> {
    calendar::submit(&ctx, event).await
}
