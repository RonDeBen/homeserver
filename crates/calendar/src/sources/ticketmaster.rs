//! Ticketmaster Discovery API source: geo-scoped event search. Reusable like
//! the ICS/RSS adapters — one impl, configured per `[[source]]` — so "music
//! near Shreveport" is a `params` block, not new code.
//!
//! Config (`params` in `sources.toml`, all non-secret):
//!   latlong             "32.5252,-93.7502"  (required)
//!   radius              search radius, default "50"
//!   unit                "miles" | "km", default "miles"
//!   classification_name segment/genre filter, default "music"
//!   keyword             optional free-text filter
//!   size                page size, default "100" (TM caps at 199)
//!
//! The API key is read from the `TICKETMASTER_API_KEY` env var at fetch time —
//! never from the committed TOML. A missing key fails only *this* source's
//! fetch (logged and skipped by the refresh loop), not the whole pass.
//!
//! Get a free key at <https://developer.ticketmaster.com/>.

use super::http;
use super::EventSource;
use crate::CalendarEvent;
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use tracing::warn;

const ENDPOINT: &str = "https://app.ticketmaster.com/discovery/v2/events.json";

pub struct TicketmasterSource {
    id: String,
    latlong: String,
    radius: String,
    unit: String,
    classification_name: String,
    keyword: Option<String>,
    size: String,
}

impl TicketmasterSource {
    /// Build from a spec's `params`. Errors if the one required knob (`latlong`)
    /// is missing — a config mistake, caught at load time.
    pub fn from_spec(spec: &super::SourceSpec) -> Result<Self> {
        let p = &spec.params;
        Ok(Self {
            id: spec.id.clone(),
            latlong: param(p, "latlong").with_context(|| {
                format!("ticketmaster source {:?} needs params.latlong", spec.id)
            })?,
            radius: param(p, "radius").unwrap_or_else(|| "50".into()),
            unit: param(p, "unit").unwrap_or_else(|| "miles".into()),
            classification_name: param(p, "classification_name").unwrap_or_else(|| "music".into()),
            keyword: param(p, "keyword"),
            size: param(p, "size").unwrap_or_else(|| "100".into()),
        })
    }
}

#[async_trait]
impl EventSource for TicketmasterSource {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&self) -> Result<Vec<CalendarEvent>> {
        let api_key = std::env::var("TICKETMASTER_API_KEY")
            .context("TICKETMASTER_API_KEY is not set (required for the ticketmaster source)")?;

        let mut query = vec![
            ("apikey", api_key.as_str()),
            ("latlong", self.latlong.as_str()),
            ("radius", self.radius.as_str()),
            ("unit", self.unit.as_str()),
            ("classificationName", self.classification_name.as_str()),
            ("size", self.size.as_str()),
            ("sort", "date,asc"),
        ];
        if let Some(k) = &self.keyword {
            query.push(("keyword", k.as_str()));
        }

        let body = http::get_text_query(ENDPOINT, &query).await?;
        parse(&body)
    }
}

/// Read a `params` value as a string, coercing numbers (TOML `radius = 50`) so
/// the file can be written naturally.
fn param(params: &toml::Table, key: &str) -> Option<String> {
    match params.get(key)? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

// --- Response shape (only the fields we use) --------------------------------

#[derive(Deserialize)]
struct Resp {
    #[serde(rename = "_embedded", default)]
    embedded: Option<Embedded>,
}

#[derive(Deserialize, Default)]
struct Embedded {
    #[serde(default)]
    events: Vec<TmEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TmEvent {
    id: String,
    name: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    please_note: Option<String>,
    dates: Dates,
    #[serde(rename = "_embedded", default)]
    embedded: Option<EventEmbedded>,
}

#[derive(Deserialize)]
struct Dates {
    start: Start,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    /// Full instant, e.g. "2026-09-01T01:00:00Z". Present when the time is known.
    #[serde(default)]
    date_time: Option<String>,
    /// Local calendar date, always present.
    #[serde(default)]
    local_date: Option<String>,
    /// Local wall-clock time; absent for date-only ("time TBA") events.
    #[serde(default)]
    local_time: Option<String>,
}

#[derive(Deserialize, Default)]
struct EventEmbedded {
    #[serde(default)]
    venues: Vec<Venue>,
}

#[derive(Deserialize)]
struct Venue {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    city: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: Option<String>,
}

/// Parse a Discovery API response body into events. `self`-free for tests.
fn parse(body: &str) -> Result<Vec<CalendarEvent>> {
    let resp: Resp = serde_json::from_str(body).context("parsing Ticketmaster response")?;
    let events = resp.embedded.unwrap_or_default().events;

    let mut out = Vec::new();
    for ev in events {
        let Some(starts_at) = parse_start(&ev.dates) else {
            warn!(id = %ev.id, name = %ev.name, "Ticketmaster event with no parseable start, skipping");
            continue;
        };
        // TODO: Ticketmaster lists some shows twice — a real event plus an
        // "Official Caesars Ticket + Hotel Packages" upsell with its own id, so
        // uid-dedup keeps both. Consider filtering the package listings once we
        // decide how (title keyword vs. something sturdier). Left as-is for now
        // to avoid overfitting to one venue's naming.
        let location = ev
            .embedded
            .as_ref()
            .and_then(|e| e.venues.first())
            .and_then(venue_label);

        out.push(CalendarEvent {
            source: String::new(), // stamped by the refresh loop
            uid: Some(ev.id),      // TM's event id is stable -> clean dedup
            title: ev.name,
            starts_at,
            ends_at: None,
            location,
            url: ev.url,
            description: ev.info.or(ev.please_note).filter(|s| !s.is_empty()),
        });
    }
    Ok(out)
}

/// Prefer the exact `dateTime` instant; fall back to local date/time in the
/// event's timezone; last resort, an all-day date at local midnight.
fn parse_start(dates: &Dates) -> Option<DateTime<Utc>> {
    if let Some(dt) = &dates.start.date_time {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(dt) {
            return Some(parsed.with_timezone(&Utc));
        }
    }

    let tz: Tz = dates
        .timezone
        .as_deref()
        .and_then(|z| z.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let date = NaiveDate::parse_from_str(dates.start.local_date.as_deref()?, "%Y-%m-%d").ok()?;
    let naive = match &dates.start.local_time {
        Some(t) => {
            let time = NaiveTime::parse_from_str(t, "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(t, "%H:%M"))
                .ok()?;
            date.and_time(time)
        }
        None => date.and_hms_opt(0, 0, 0)?,
    };
    tz.from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

/// "Venue, City" from whatever venue fields are present.
fn venue_label(v: &Venue) -> Option<String> {
    let name = v.name.as_deref().filter(|s| !s.is_empty());
    let city = v
        .city
        .as_ref()
        .and_then(|c| c.name.as_deref())
        .filter(|s| !s.is_empty());
    match (name, city) {
        (Some(n), Some(c)) => Some(format!("{n}, {c}")),
        (Some(n), None) => Some(n.to_owned()),
        (None, Some(c)) => Some(c.to_owned()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
      "_embedded": { "events": [
        {
          "id": "G5v0Z9Yabc",
          "name": "Some Touring Metal Band",
          "url": "https://www.ticketmaster.com/event/G5v0Z9Yabc",
          "info": "Doors at 7.",
          "dates": { "start": { "dateTime": "2026-09-01T01:00:00Z", "localDate": "2026-08-31", "localTime": "20:00:00" }, "timezone": "America/Chicago" },
          "_embedded": { "venues": [ { "name": "Municipal Auditorium", "city": { "name": "Shreveport" } } ] }
        },
        {
          "id": "TBA123",
          "name": "Date-only Festival",
          "dates": { "start": { "localDate": "2026-09-05" }, "timezone": "America/Chicago" }
        }
      ] }
    }"#;

    #[test]
    fn parses_datetime_event_with_venue() {
        let events = parse(JSON).unwrap();
        let e = &events[0];
        assert_eq!(e.uid.as_deref(), Some("G5v0Z9Yabc"));
        assert_eq!(e.title, "Some Touring Metal Band");
        assert_eq!(e.starts_at.to_rfc3339(), "2026-09-01T01:00:00+00:00");
        assert_eq!(
            e.location.as_deref(),
            Some("Municipal Auditorium, Shreveport")
        );
        assert_eq!(
            e.url.as_deref(),
            Some("https://www.ticketmaster.com/event/G5v0Z9Yabc")
        );
        assert_eq!(e.description.as_deref(), Some("Doors at 7."));
    }

    #[test]
    fn parses_date_only_event_in_timezone() {
        let events = parse(JSON).unwrap();
        let e = &events[1];
        assert_eq!(e.title, "Date-only Festival");
        // Midnight America/Chicago on 2026-09-05 is 05:00 UTC (CDT, -05:00).
        assert_eq!(e.starts_at.to_rfc3339(), "2026-09-05T05:00:00+00:00");
    }
}
