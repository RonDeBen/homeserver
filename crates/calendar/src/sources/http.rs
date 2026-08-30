//! Shared HTTP client for source fetches. One place to set the timeout, the
//! User-Agent, and (later) any retry/backoff, so every adapter behaves the same
//! and we're a well-mannered scraper.

use anyhow::{Context, Result};
use std::sync::OnceLock;
use std::time::Duration;

/// Identifies us to the sites we pull from — polite, and easy to allowlist.
const USER_AGENT: &str = concat!("homeserver-calendar/", env!("CARGO_PKG_VERSION"));

/// Cap on any single fetch. A hung source shouldn't stall the refresh pass;
/// this bounds how long one bad feed can hold things up before it's skipped.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Process-wide client, built once. `reqwest::Client` is a pool over an `Arc`,
/// so sharing it reuses connections across sources and refresh passes.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .expect("build reqwest client")
    })
}

/// GET `url` and return the body as text, erroring on any non-success status.
/// The one call every adapter routes through to pull bytes off the network.
pub async fn get_text(url: &str) -> Result<String> {
    get_text_query(url, &[]).await
}

/// Like [`get_text`] but with URL query parameters, which `reqwest` encodes for
/// us — the safe way to attach an API key and search filters. The API adapters
/// (Ticketmaster) use this; the feed/scrape ones just call [`get_text`].
pub async fn get_text_query(url: &str, query: &[(&str, &str)]) -> Result<String> {
    let resp = client()
        .get(url)
        .query(query)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("bad status from {url}"))?;
    resp.text()
        .await
        .with_context(|| format!("reading body from {url}"))
}
