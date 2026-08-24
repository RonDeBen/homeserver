//! `hsctl` — homeserver control. A message injector for poking the system by
//! hand and testing jobs end-to-end. It only talks to the bus (no database).
//!
//! Typed subcommands publish real [`common::Event`] types (borrowed from the
//! owning job's crate, so the contract can't drift); `fire` is the raw escape
//! hatch for any subject. To watch what comes back, use the `nats` CLI, e.g.
//! `nats sub 'calendar.>'`.
//!
//! ```text
//! hsctl calendar refresh
//! hsctl calendar submit "Dentist" 2026-09-01T09:00:00Z --location "Main St"
//! hsctl fire some.subject '{"any":"json"}'
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use common::Event;

#[derive(Parser)]
#[command(about = "homeserver control — inject messages onto the bus")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Publish a raw JSON payload to any subject.
    Fire {
        subject: String,
        /// JSON payload (default: empty object).
        #[arg(default_value = "{}")]
        payload: String,
    },
    /// Schedule a durable one-shot timer that fires after a delay.
    Timer {
        /// Delay before firing, e.g. 10s, 5m, 1h30m.
        #[arg(value_parser = humantime::parse_duration)]
        delay: std::time::Duration,
        /// Subject to publish when it fires.
        subject: String,
        /// JSON payload (default: empty object).
        #[arg(default_value = "{}")]
        payload: String,
    },
    /// Calendar messages.
    #[command(subcommand)]
    Calendar(CalendarCmd),
}

#[derive(Subcommand)]
enum CalendarCmd {
    /// Trigger a refresh/scrape pass (what the orchestrator sends on a cron).
    Refresh,
    /// Submit a calendar event by hand, as a person would.
    Submit {
        title: String,
        /// Start time, RFC3339, e.g. 2026-09-01T09:00:00Z
        starts_at: DateTime<Utc>,
        #[arg(long)]
        location: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let bus = common::connect_bus().await?;

    match cli.command {
        Command::Fire { subject, payload } => {
            let value: serde_json::Value =
                serde_json::from_str(&payload).context("payload must be valid JSON")?;
            common::bus::publish_json(&bus, &subject, &value).await?;
            println!("fired  {subject}  {value}");
        }
        Command::Timer {
            delay,
            subject,
            payload,
        } => {
            let value: serde_json::Value =
                serde_json::from_str(&payload).context("payload must be valid JSON")?;
            let fire_at = Utc::now() + chrono::Duration::from_std(delay)?;
            let timer = orchestrator::TimerCreate {
                subject: subject.clone(),
                payload: value,
                fire_at,
            };
            common::bus::emit(&bus, &timer).await?;
            println!("timer set: {subject} fires at {fire_at}");
        }
        Command::Calendar(CalendarCmd::Refresh) => {
            common::bus::emit(&bus, &calendar::CalendarRefresh::default()).await?;
            println!("fired  {}", calendar::CalendarRefresh::SUBJECT);
        }
        Command::Calendar(CalendarCmd::Submit {
            title,
            starts_at,
            location,
        }) => {
            let event = calendar::UserSubmittedEvent {
                title,
                starts_at,
                location,
            };
            common::bus::emit(&bus, &event).await?;
            println!(
                "fired  {}  {}",
                calendar::UserSubmittedEvent::SUBJECT,
                event.title
            );
        }
    }
    Ok(())
}
