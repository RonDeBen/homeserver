//! Gateway job — the HTTP surface for humans (web now, iOS later).
//!
//! Like every other job it's a bus participant with a Postgres pool
//! (`common::init`), but instead of a `Subscriptions` router it listens on HTTP.
//! It's a *backend-for-frontend*: it reads Postgres for queries and subscribes
//! to the bus to push live updates to connected clients. The work lives in
//! `lib.rs`; this binary is the thin shell.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = common::init().await?;
    // Idempotent, safe on every startup — same discipline as the other jobs.
    common::db::migrate(&ctx.db).await?;
    gateway::run(ctx).await
}
