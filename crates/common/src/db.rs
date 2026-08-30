use anyhow::{Context, Result};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;
use tracing::warn;

/// Open a pooled Postgres connection, retrying briefly while Postgres comes up.
///
/// Infra and jobs start together (compose / a reboot), so the DB may not be
/// accepting connections yet on the first try. We retry with a fixed delay,
/// then give up and let the process crash — a supervisor (compose
/// `restart: unless-stopped`, or systemd) brings it back. NATS needs no
/// equivalent: async-nats reconnects on its own.
pub async fn connect(database_url: &str) -> Result<PgPool> {
    const MAX_ATTEMPTS: u32 = 10;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
        {
            Ok(pool) => return Ok(pool),
            Err(e) if attempt < MAX_ATTEMPTS => {
                warn!(attempt, error = %e, "postgres not ready, retrying in 1s");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e).context("connecting to postgres (gave up after retries)"),
        }
    }
}

/// Bring the shared schema up to date. Idempotent and checksum-verified by
/// sqlx, so it's safe to call on every job's startup. The path is resolved at
/// compile time relative to this crate (`crates/common` -> repo `/migrations`),
/// so jobs no longer need to know where migrations live.
pub async fn migrate(db: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations").run(db).await?;
    Ok(())
}
