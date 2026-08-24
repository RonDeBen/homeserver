//! Shared plumbing every job reuses: config, DB pool, NATS client, logging.
//!
//! A job starts with `let ctx = common::init().await?;` and then reaches for
//! `ctx.db` and `ctx.bus`. Keeping this in one crate means jobs don't each
//! reinvent connection setup or logging.

pub mod bus;
pub mod config;
pub mod db;
pub mod logging;

pub use bus::Event;
pub use config::Config;

use anyhow::Result;

/// Everything a job needs to talk to the rest of the system.
pub struct Context {
    pub config: Config,
    pub db: sqlx::PgPool,
    pub bus: async_nats::Client,
}

/// Load env + logging, then connect to Postgres and NATS.
///
/// Fine to call at the top of any job's `main`.
pub async fn init() -> Result<Context> {
    let _ = dotenvy::dotenv();
    logging::init();

    let config = Config::from_env()?;
    let db = db::connect(&config.database_url).await?;
    let bus = bus::connect(&config.nats_url).await?;

    Ok(Context { config, db, bus })
}

/// Connect to NATS only — no database. For bus-only tools like `hsctl` that
/// inject messages and don't touch Postgres.
pub async fn connect_bus() -> Result<async_nats::Client> {
    let _ = dotenvy::dotenv();
    logging::init();
    let config = Config::from_env()?;
    bus::connect(&config.nats_url).await
}

/// Resolves when the process is asked to stop (Ctrl-C / SIGINT, or SIGTERM
/// from Docker/systemd). Daemons `tokio::select!` on this to shut down cleanly;
/// `common::bus::serve` already does.
pub async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
