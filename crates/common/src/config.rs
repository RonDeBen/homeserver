use anyhow::{Context, Result};

/// Runtime config, read from the environment (see `.env.example`).
///
/// Every value is required: a missing one is a misconfiguration, so we fail
/// fast at startup rather than silently falling back to a default that happens
/// to work on one machine. For local dev, `cp .env.example .env`.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: require("DATABASE_URL")?,
            nats_url: require("NATS_URL")?,
        })
    }
}

/// A required env var. Absent = fatal, with a message that names the var.
fn require(key: &str) -> Result<String> {
    std::env::var(key)
        .with_context(|| format!("required env var {key} is not set (see .env.example)"))
}
