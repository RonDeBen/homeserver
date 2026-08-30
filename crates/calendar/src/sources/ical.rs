//! Generic iCalendar (`.ics`) source. Fully config-driven: point it at any ICS
//! URL and it works — no per-site code. This is the reusable adapter the
//! library and university feeds share.

use super::http;
use super::EventSource;
use crate::CalendarEvent;
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use ical::property::Property;
use tracing::warn;

/// An ICS feed at a URL. `id` is stamped onto every event and used for dedup.
pub struct IcalSource {
    id: String,
    url: String,
}

impl IcalSource {
    pub fn new(id: String, url: String) -> Self {
        Self { id, url }
    }
}

#[async_trait]
impl EventSource for IcalSource {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&self) -> Result<Vec<CalendarEvent>> {
        let body = http::get_text(&self.url).await?;
        parse(&body)
    }
}

/// Parse an ICS document into events. Kept free of `self` so it's unit-testable
/// against fixture strings without any network.
fn parse(body: &str) -> Result<Vec<CalendarEvent>> {
    let mut events = Vec::new();
    for calendar in ical::IcalParser::new(body.as_bytes()) {
        let calendar = calendar.context("parsing ICS")?;
        for vevent in calendar.events {
            // An event with no title or no start is unusable; skip it loudly
            // rather than storing a junk row.
            let Some(event) = event_from_vevent(&vevent.properties) else {
                warn!("skipping VEVENT with no SUMMARY or unparseable DTSTART");
                continue;
            };
            events.push(event);
        }
    }
    Ok(events)
}

/// Build a [`CalendarEvent`] from a VEVENT's properties. Returns `None` if the
/// event lacks the two things we can't do without: a title and a start time.
/// `source` is left empty — the refresh loop stamps it from the source id.
fn event_from_vevent(props: &[Property]) -> Option<CalendarEvent> {
    let title = unescape(prop(props, "SUMMARY")?);
    let starts_at = prop_datetime(props, "DTSTART")?;

    Some(CalendarEvent {
        source: String::new(),
        uid: prop(props, "UID").map(str::to_owned),
        title,
        starts_at,
        ends_at: prop_datetime(props, "DTEND"),
        location: prop(props, "LOCATION")
            .map(unescape)
            .filter(|s| !s.is_empty()),
        url: prop(props, "URL").map(str::to_owned),
        description: prop(props, "DESCRIPTION")
            .map(unescape)
            .filter(|s| !s.is_empty()),
    })
}

/// Value of the first property named `name` (case-insensitive), if present and
/// non-empty.
fn prop<'a>(props: &'a [Property], name: &str) -> Option<&'a str> {
    props
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .and_then(|p| p.value.as_deref())
        .filter(|v| !v.is_empty())
}

/// Parse a date/time property (DTSTART/DTEND) to UTC, honoring its form:
///   * `...Z`            -> UTC
///   * `TZID=...` param  -> that zone, converted to UTC
///   * 8 chars (date)    -> all-day; midnight UTC (see note)
///   * otherwise         -> "floating" local time; best-effort as UTC
fn prop_datetime(props: &[Property], name: &str) -> Option<DateTime<Utc>> {
    let property = props.iter().find(|p| p.name.eq_ignore_ascii_case(name))?;
    let value = property.value.as_deref()?.trim();

    // UTC, e.g. 20260901T090000Z.
    if let Some(stripped) = value.strip_suffix('Z') {
        return NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S")
            .ok()
            .map(|dt| Utc.from_utc_datetime(&dt));
    }

    // Zoned, e.g. DTSTART;TZID=America/Chicago:20260901T090000.
    if let Some(tz) = tzid(property).and_then(|z| z.parse::<Tz>().ok()) {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S") {
            // `.single()` skips DST gaps/overlaps rather than guessing.
            return tz
                .from_local_datetime(&naive)
                .single()
                .map(|dt| dt.with_timezone(&Utc));
        }
    }

    // All-day (VALUE=DATE), e.g. 20260901. Anchored to midnight UTC — good
    // enough for "what's on that day"; refine with a per-source tz if a
    // real query ever needs local-midnight precision.
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y%m%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|dt| Utc.from_utc_datetime(&dt));
    }

    // Floating local time with no zone: treat as UTC (best effort).
    NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
        .ok()
        .map(|dt| Utc.from_utc_datetime(&dt))
}

/// The `TZID` parameter value of a property, if any.
fn tzid(property: &Property) -> Option<&str> {
    property.params.as_ref()?.iter().find_map(|(k, v)| {
        (k.eq_ignore_ascii_case("TZID"))
            .then(|| v.first().map(String::as_str))
            .flatten()
    })
}

/// Undo RFC 5545 text escaping: `\n`/`\N` -> newline, and `\,` `\;` `\\` ->
/// their literal characters.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other), // \, \; \\ and any stray escape
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:evt-1@example.org
SUMMARY:Book Club: The Overstory
DTSTART:20260901T180000Z
DTEND:20260901T193000Z
LOCATION:Main Branch\\, Room 2
URL:https://example.org/events/1
DESCRIPTION:Monthly meetup.\\nBring a book.
END:VEVENT
BEGIN:VEVENT
UID:evt-2@example.org
SUMMARY:All-day Fair
DTSTART;VALUE=DATE:20260903
END:VEVENT
END:VCALENDAR
";

    #[test]
    fn parses_utc_event_with_all_fields() {
        let events = parse(SAMPLE).unwrap();
        let e = &events[0];
        assert_eq!(e.uid.as_deref(), Some("evt-1@example.org"));
        assert_eq!(e.title, "Book Club: The Overstory");
        assert_eq!(e.starts_at.to_rfc3339(), "2026-09-01T18:00:00+00:00");
        assert_eq!(e.ends_at.unwrap().to_rfc3339(), "2026-09-01T19:30:00+00:00");
        assert_eq!(e.location.as_deref(), Some("Main Branch, Room 2"));
        assert_eq!(e.url.as_deref(), Some("https://example.org/events/1"));
        assert_eq!(
            e.description.as_deref(),
            Some("Monthly meetup.\nBring a book.")
        );
    }

    #[test]
    fn parses_all_day_date_only_event() {
        let events = parse(SAMPLE).unwrap();
        let e = &events[1];
        assert_eq!(e.title, "All-day Fair");
        assert_eq!(e.starts_at.to_rfc3339(), "2026-09-03T00:00:00+00:00");
        assert!(e.ends_at.is_none());
    }

    #[test]
    fn honors_tzid() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:z\nSUMMARY:Zoned\n\
DTSTART;TZID=America/Chicago:20260901T090000\nEND:VEVENT\nEND:VCALENDAR\n";
        let events = parse(ics).unwrap();
        // 09:00 CDT (UTC-5 in September) == 14:00 UTC.
        assert_eq!(
            events[0].starts_at.to_rfc3339(),
            "2026-09-01T14:00:00+00:00"
        );
    }
}
