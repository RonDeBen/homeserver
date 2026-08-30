//! Generic RSS/Atom source. Config-driven like [`super::ical`]: point it at a
//! feed URL and it works. `feed-rs` parses both RSS and Atom, so one adapter
//! covers both.
//!
//! Caveat worth knowing: RSS is a *publishing* format, not a calendar one. An
//! entry carries a publish date, not an event start/end, and has no location
//! field. So this adapter maps `published` -> `starts_at` as a best effort;
//! it's fine for discovery feeds (SBFunGuide) where "roughly when it went up"
//! is the only time signal, but a site with real event times is better served
//! by ICS or a purpose-built HTML adapter.

use super::http;
use super::EventSource;
use crate::CalendarEvent;
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use tracing::warn;

/// An RSS/Atom feed at a URL.
pub struct RssSource {
    id: String,
    url: String,
}

impl RssSource {
    pub fn new(id: String, url: String) -> Self {
        Self { id, url }
    }
}

#[async_trait]
impl EventSource for RssSource {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&self) -> Result<Vec<CalendarEvent>> {
        let body = http::get_text(&self.url).await?;
        parse(&body)
    }
}

/// Parse a feed document into events. `self`-free for fixture-based tests.
fn parse(body: &str) -> Result<Vec<CalendarEvent>> {
    let feed = feed_rs::parser::parse(body.as_bytes()).context("parsing RSS/Atom feed")?;
    let mut events = Vec::new();
    for entry in feed.entries {
        // Need a title and *some* time to be a usable event.
        let Some(title) = entry.title.map(|t| t.content).filter(|t| !t.is_empty()) else {
            warn!("skipping feed entry with no title");
            continue;
        };
        let Some(starts_at) = entry.published.or(entry.updated) else {
            warn!(%title, "skipping feed entry with no published/updated date");
            continue;
        };

        events.push(CalendarEvent {
            source: String::new(), // stamped by the refresh loop
            // feed-rs always synthesizes a stable `id`; use it for dedup so a
            // re-titled entry doesn't duplicate.
            uid: (!entry.id.is_empty()).then_some(entry.id),
            title,
            starts_at,
            ends_at: None,
            location: None,
            url: entry.links.into_iter().next().map(|l| l.href),
            description: entry.summary.map(|t| t.content).filter(|s| !s.is_empty()),
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>SB Fun Guide</title>
  <item>
    <title>Downtown Festival</title>
    <link>https://example.org/festival</link>
    <description>Food, music, art.</description>
    <guid>festival-2026</guid>
    <pubDate>Mon, 01 Sep 2026 12:00:00 GMT</pubDate>
  </item>
</channel></rss>"#;

    #[test]
    fn parses_rss_item() {
        let events = parse(RSS).unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.title, "Downtown Festival");
        assert_eq!(e.url.as_deref(), Some("https://example.org/festival"));
        assert_eq!(e.description.as_deref(), Some("Food, music, art."));
        assert_eq!(e.starts_at.to_rfc3339(), "2026-09-01T12:00:00+00:00");
        assert!(e.uid.is_some());
    }
}
