use tracing_subscriber::{fmt, EnvFilter};

/// Structured logging, level controlled by `RUST_LOG` (default `info`).
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // `try_init` so calling this twice in tests doesn't panic.
    let _ = fmt().with_env_filter(filter).try_init();
}
