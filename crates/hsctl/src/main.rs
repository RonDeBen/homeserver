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
    /// Health messages (fasting, weigh-ins, lifting).
    #[command(subcommand)]
    Health(HealthCmd),
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

#[derive(Subcommand)]
enum HealthCmd {
    /// Intermittent-fasting controls.
    #[command(subcommand)]
    Fast(FastCmd),
    /// Record a weigh-in, in pounds.
    Weight {
        lbs: f64,
        /// Measurement time, RFC3339 (default: now).
        #[arg(long)]
        at: Option<DateTime<Utc>>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Record one lifting set.
    Lift {
        exercise: String,
        lbs: f64,
        reps: i32,
        /// When performed, RFC3339 (default: now).
        #[arg(long)]
        at: Option<DateTime<Utc>>,
    },
}

#[derive(Subcommand)]
enum FastCmd {
    /// Start a fast, optionally with a target duration.
    Start {
        /// Start time, RFC3339 (default: now).
        #[arg(long)]
        at: Option<DateTime<Utc>>,
        /// Target fast length in hours (e.g. 16).
        #[arg(long)]
        target: Option<f64>,
        /// Named preset that maps to target hours, e.g. 16:8, 18:6, omad.
        #[arg(long)]
        preset: Option<String>,
    },
    /// End the current open fast.
    End {
        /// End time, RFC3339 (default: now).
        #[arg(long)]
        at: Option<DateTime<Utc>>,
    },
}

/// Map a named fasting preset to target hours. `H:R` presets take the fasting
/// hours (the first number); `omad` is one-meal-a-day (~23h).
fn preset_hours(preset: &str) -> Result<f64> {
    match preset.to_ascii_lowercase().as_str() {
        "omad" => Ok(23.0),
        other => other
            .split_once(':')
            .and_then(|(fast, _eat)| fast.trim().parse::<f64>().ok())
            .with_context(|| format!("unrecognized fast preset {preset:?} (try 16:8, 18:6, omad)")),
    }
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
        Command::Health(HealthCmd::Fast(FastCmd::Start { at, target, preset })) => {
            // --target wins over --preset when both are given.
            let target_hours = match (target, preset) {
                (Some(t), _) => Some(t),
                (None, Some(p)) => Some(preset_hours(&p)?),
                (None, None) => None,
            };
            let event = health::FastStarted {
                started_at: at.unwrap_or_else(Utc::now),
                target_hours,
            };
            common::bus::emit(&bus, &event).await?;
            match target_hours {
                Some(h) => println!("fired  {}  target {h}h", health::FastStarted::SUBJECT),
                None => println!("fired  {}  (no target)", health::FastStarted::SUBJECT),
            }
        }
        Command::Health(HealthCmd::Fast(FastCmd::End { at })) => {
            let event = health::FastEnded {
                ended_at: at.unwrap_or_else(Utc::now),
            };
            common::bus::emit(&bus, &event).await?;
            println!("fired  {}", health::FastEnded::SUBJECT);
        }
        Command::Health(HealthCmd::Weight { lbs, at, note }) => {
            let event = health::WeightRecorded {
                measured_at: at.unwrap_or_else(Utc::now),
                weight_lbs: lbs,
                note,
            };
            common::bus::emit(&bus, &event).await?;
            println!("fired  {}  {lbs} lbs", health::WeightRecorded::SUBJECT);
        }
        Command::Health(HealthCmd::Lift {
            exercise,
            lbs,
            reps,
            at,
        }) => {
            let event = health::LiftRecorded {
                performed_at: at.unwrap_or_else(Utc::now),
                exercise,
                weight_lbs: lbs,
                reps,
                note: None,
            };
            common::bus::emit(&bus, &event).await?;
            println!(
                "fired  {}  {} {lbs}x{reps}",
                health::LiftRecorded::SUBJECT,
                event.exercise
            );
        }
    }
    Ok(())
}
