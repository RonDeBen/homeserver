//! Per-site HTML adapter: Bear's on Fairfield (metal/punk/noise/DIY shows).
//!
//! This is also the **template** for every scraped site: the selectors and the
//! date format are specific to this one page and live in this one file. To add
//! another scraped site, copy this module, adjust the selectors + `parse_when`,
//! and add an arm to `sources::build`.
//!
//! How the page is shaped (a Duda site): each show is a `div.flex-element.group`
//! holding, as siblings, an `<h4>` title, a paragraph like
//! `Sat, Aug 1 at 8:00 PM CDT`, and a "Learn More" link to a Facebook event.
//! We anchor on the date paragraph (the reliable signal that a block is an
//! event), then walk up to its enclosing group to grab the title and link.
//!
//! Fragile bits, by design of the source (flag if they break):
//!   * the date text carries **no year** — we infer it (see [`resolve_year`]);
//!   * times are labeled CDT/CST but we interpret them in America/Chicago,
//!     which gets the offset right regardless of the label;
//!   * the Facebook event URL is our stable dedup id — the page itself has none.

use super::http;
use super::EventSource;
use crate::CalendarEvent;
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::America::Chicago;
use regex::Regex;
use scraper::{CaseSensitivity, ElementRef, Html, Selector};
use std::sync::OnceLock;
use tracing::warn;

pub struct BearsSource {
    id: String,
    url: String,
}

impl BearsSource {
    pub fn new(id: String, url: String) -> Self {
        Self { id, url }
    }
}

#[async_trait]
impl EventSource for BearsSource {
    fn id(&self) -> &str {
        &self.id
    }

    async fn fetch(&self) -> Result<Vec<CalendarEvent>> {
        let html = http::get_text(&self.url).await?;
        parse(&html)
    }
}

/// One event's date line, e.g. `Sat, Aug 1 at 8:00 PM CDT`. Captures:
/// 1=month, 2=day, 3=hour, 4=minute, 5=am/pm. Weekday and tz label are ignored.
fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?ix)
            (?:sun|mon|tue|wed|thu|fri|sat)\s*,\s*
            (jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\s+
            (\d{1,2})\s+at\s+
            (\d{1,2}):(\d{2})\s*(am|pm)",
        )
        .expect("valid date regex")
    })
}

/// Parse the events page. `self`-free so it's testable against a saved copy of
/// the page with no network.
fn parse(html: &str) -> Result<Vec<CalendarEvent>> {
    let paragraph = selector(".dmNewParagraph")?;
    let heading = selector("h4")?;
    let link = selector("a[href]")?;

    let doc = Html::parse_document(html);
    let mut events = Vec::new();

    // Anchor on paragraphs whose text is a date line; each marks one event.
    for para in doc.select(&paragraph) {
        let text = normalize(&para.text().collect::<String>());
        let Some(caps) = date_re().captures(&text) else {
            continue;
        };
        let Some(starts_at) = parse_when(&caps) else {
            warn!(line = %text, "Bear's: matched a date line but couldn't build a datetime");
            continue;
        };

        // The enclosing event card holds the title and the link.
        let Some(group) = nearest_group(para) else {
            continue;
        };
        let Some(title) = group
            .select(&heading)
            .next()
            .map(|h| normalize(&h.text().collect::<String>()))
            .filter(|t| !t.is_empty())
        else {
            warn!(line = %text, "Bear's: event card with a date but no <h4> title, skipping");
            continue;
        };
        // First real link in the card — the Facebook event, which doubles as a
        // stable id (the page has no other) and the click-through url.
        let href = group
            .select(&link)
            .filter_map(|a| a.value().attr("href"))
            .find(|h| h.starts_with("http"))
            .map(first_url);

        events.push(CalendarEvent {
            source: String::new(), // stamped by the refresh loop
            uid: href.clone(),
            title,
            starts_at,
            ends_at: None,
            location: None, // it's all Bear's; the `source` already says so
            url: href,
            description: None,
        });
    }
    Ok(events)
}

/// Turn a regex match of the date line into an instant. The page omits the
/// year, so we infer it in [`resolve_year`].
fn parse_when(caps: &regex::Captures) -> Option<DateTime<Utc>> {
    let month = month_num(&caps[1])?;
    let day: u32 = caps[2].parse().ok()?;
    let mut hour: u32 = caps[3].parse().ok()?;
    let minute: u32 = caps[4].parse().ok()?;
    let pm = caps[5].eq_ignore_ascii_case("pm");

    // 12-hour -> 24-hour.
    hour %= 12;
    if pm {
        hour += 12;
    }

    resolve_year(month, day, hour, minute)
}

/// Pick the year for a (month, day, time) with none given: assume this year,
/// but if that lands more than ~30 days in the past, roll to next year. Works
/// for a page of upcoming shows (with a few recently-passed ones still listed)
/// and handles the Dec->Jan boundary.
fn resolve_year(month: u32, day: u32, hour: u32, minute: u32) -> Option<DateTime<Utc>> {
    resolve_year_at(Utc::now(), month, day, hour, minute)
}

/// [`resolve_year`] with an explicit "now", so the year heuristic is testable
/// without depending on the wall clock.
fn resolve_year_at(
    now: DateTime<Utc>,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Option<DateTime<Utc>> {
    let base_year = now.with_timezone(&Chicago).year();
    for year in [base_year, base_year + 1] {
        let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, 0)?;
        // `.single()` handles the normal case; `.earliest()` breaks a DST tie.
        let Some(local) = Chicago
            .from_local_datetime(&naive)
            .single()
            .or_else(|| Chicago.from_local_datetime(&naive).earliest())
        else {
            continue;
        };
        let utc = local.with_timezone(&Utc);
        if utc >= now - Duration::days(30) {
            return Some(utc);
        }
    }
    None
}

fn month_num(abbr: &str) -> Option<u32> {
    Some(match abbr.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// Keep the first URL when a card's href carries it duplicated back-to-back
/// (`https://…/123/https://…/123/`), a quirk seen in the live markup. Cuts at
/// the second scheme so the `uid` stays stable.
fn first_url(href: &str) -> String {
    match href.match_indices("http").nth(1) {
        Some((second, _)) => href[..second].to_owned(),
        None => href.to_owned(),
    }
}

/// Nearest ancestor that is a Duda `flex-element group` — the event card.
fn nearest_group(el: ElementRef) -> Option<ElementRef> {
    el.ancestors().find_map(|node| {
        let e = ElementRef::wrap(node)?;
        let v = e.value();
        (v.has_class("flex-element", CaseSensitivity::AsciiCaseInsensitive)
            && v.has_class("group", CaseSensitivity::AsciiCaseInsensitive))
        .then_some(e)
    })
}

/// Collapse all whitespace (incl. non-breaking spaces from the HTML) to single
/// spaces and trim — so the date regex sees `Sat, Aug 1`, not `Sat,\u{a0}Aug 1`.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compile a CSS selector, turning `scraper`'s non-`Send` parse error into an
/// `anyhow` one that names the bad selector.
fn selector(sel: &str) -> Result<Selector> {
    Selector::parse(sel)
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .with_context(|| format!("invalid CSS selector {sel:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like the real Duda markup: an event card is a flex-element group
    // with an <h4> title, a .dmNewParagraph date line, and a Facebook link.
    const PAGE: &str = r#"
      <div class="flex-element group">
        <div class="dmNewParagraph"><h4><span>Dana Ives &bull; Neutral Snap</span></h4></div>
        <div class="dmNewParagraph"><p><span>Sat,&nbsp;Aug 1 at 8:00 PM CDT</span></p></div>
        <a class="dmButtonLink" href="https://www.facebook.com/events/865412906287951/">Learn More</a>
      </div>
      <div class="flex-element group">
        <div class="dmHoursOfOperation"><dt>Sun - Thu</dt><time>5:00 pm</time></div>
      </div>
    "#;

    #[test]
    fn scrapes_event_and_ignores_hours_widget() {
        let events = parse(PAGE).unwrap();
        assert_eq!(events.len(), 1, "hours widget must not be read as an event");
        let e = &events[0];
        assert_eq!(e.title, "Dana Ives • Neutral Snap");
        // 20:00 CDT (America/Chicago, -05:00) == 01:00 UTC next day. Year-agnostic
        // (it depends on the wall clock via resolve_year); that logic is pinned
        // in `year_rolls_over_when_date_is_well_past`.
        assert_eq!(e.starts_at.format("%m-%dT%H:%M").to_string(), "08-02T01:00");
        assert_eq!(
            e.uid.as_deref(),
            Some("https://www.facebook.com/events/865412906287951/")
        );
        assert_eq!(e.url, e.uid);
    }

    #[test]
    fn first_url_dedupes_doubled_href() {
        assert_eq!(
            first_url("https://x.com/e/1/https://x.com/e/1/"),
            "https://x.com/e/1/"
        );
        assert_eq!(first_url("https://x.com/e/1/"), "https://x.com/e/1/");
    }

    #[test]
    fn year_rolls_over_when_date_is_well_past() {
        // "now" = 2026-12-15. A "Jan 3" show is next year, not a year ago.
        // 20:00 CST (-06:00) is 02:00 UTC on Jan 4.
        let now = Utc.with_ymd_and_hms(2026, 12, 15, 12, 0, 0).unwrap();
        let dt = resolve_year_at(now, 1, 3, 20, 0).unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2027-01-04");

        // A date only a couple weeks back stays in the current year (recently
        // passed shows still linger on the page).
        let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
        let dt = resolve_year_at(now, 8, 1, 20, 0).unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-08-02"); // +1d in UTC
    }
}
