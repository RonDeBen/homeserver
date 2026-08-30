//! Calendar sources: the facade that lets one refresh pass pull from many
//! heterogeneous places (ICS feeds, RSS, scraped HTML) through one interface.
//!
//! The shape, top to bottom:
//!
//! - [`EventSource`] — the trait every source implements: an `id` and an async
//!   `fetch` that returns normalized [`CalendarEvent`]s. This is the seam;
//!   `refresh` never knows whether it's talking to ICS or a hand-written scraper.
//! - [`SourceSpec`] — one `[[source]]` entry from `deploy/sources.toml`. The
//!   file is the source of truth; adding a source is a diffable edit there.
//! - [`build`] — the registry/factory: turns a spec into a `Box<dyn EventSource>`
//!   by `kind`. `ical`/`rss` are reusable and fully config-driven (URL is all
//!   they need); `html` dispatches on `adapter` to a per-site impl.
//!
//! Adding a source:
//!   * an ICS or RSS feed  -> just add a `[[source]]` entry, no code.
//!   * an ugly HTML page   -> write a small adapter in `sources/`, implement
//!     [`EventSource`], and add one arm to [`build`]'s `html` match.

mod http;
mod ical;
mod rss;
mod ticketmaster;

// Per-site HTML adapters. One module per scraped site; each is the whole story
// of how to read that one page. Add a `mod foo;` here + an arm in `build`.
mod bears;

use crate::CalendarEvent;
use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use serde::Deserialize;

pub use ical::IcalSource;
pub use rss::RssSource;
pub use ticketmaster::TicketmasterSource;

/// A place events come from. Implementors normalize whatever they fetch into
/// [`CalendarEvent`]s; the `source` field on each event is stamped by the
/// refresh loop from [`EventSource::id`], so adapters can leave it empty.
#[async_trait]
pub trait EventSource: Send + Sync {
    /// Stable, unique id for this source (e.g. `"shreve-library"`). Also stored
    /// as the `source` column on every event, and used in the dedup key — so it
    /// must be stable across runs.
    fn id(&self) -> &str;

    /// Fetch and normalize this source's current events. Errors bubble up; the
    /// refresh loop logs and skips a failing source rather than aborting the
    /// whole pass, so one dead feed never blocks the others.
    async fn fetch(&self) -> Result<Vec<CalendarEvent>>;
}

/// One `[[source]]` entry from `sources.toml`.
///
/// `kind` selects the adapter. `url` is required by the fetch-a-document kinds
/// (`ical`/`rss`/`html`) and unused by API kinds that know their own endpoint
/// (`ticketmaster`). `adapter` names the per-site scraper for `kind = "html"`.
/// `params` is a free-form table for adapter-specific, *non-secret* config
/// (Ticketmaster's lat/long, radius, genres); secrets come from env, never here.
#[derive(Debug, Deserialize)]
pub struct SourceSpec {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub kind: SourceKind,
    #[serde(default)]
    pub url: Option<String>,
    /// For `kind = "html"`: which per-site adapter to use. Ignored otherwise.
    #[serde(default)]
    pub adapter: Option<String>,
    /// Adapter-specific knobs. Interpreted by the adapter; ignored if it has none.
    #[serde(default)]
    pub params: toml::Table,
}

impl SourceSpec {
    /// The `url`, erroring with the source id if a kind that needs one omitted it.
    fn require_url(&self) -> Result<String> {
        self.url.clone().with_context(|| {
            format!(
                "source {:?} (kind={:?}) requires a `url`",
                self.id, self.kind
            )
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// iCalendar feed. Generic — works against any ICS URL.
    Ical,
    /// RSS or Atom feed. Generic — `feed-rs` handles both.
    Rss,
    /// A scraped HTML page. Needs a per-site `adapter`.
    Html,
    /// Ticketmaster Discovery API: geo-scoped event search. Reusable like
    /// `ical`/`rss`; configured via `params` + the `TICKETMASTER_API_KEY` env.
    Ticketmaster,
}

#[derive(Debug, Deserialize)]
struct SourcesFile {
    #[serde(default)]
    source: Vec<SourceSpec>,
}

/// Parse `sources.toml` into specs.
pub fn load_specs(path: &str) -> Result<Vec<SourceSpec>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading sources file {path}"))?;
    let file: SourcesFile =
        toml::from_str(&text).with_context(|| format!("parsing sources file {path}"))?;
    Ok(file.source)
}

/// The registry/factory: build the concrete source for a spec.
///
/// Generic kinds (`ical`, `rss`) are constructed straight from the URL. `html`
/// dispatches on `adapter` to a hand-written per-site impl — that `match` is the
/// list of scrapers we have.
pub fn build(spec: SourceSpec) -> Result<Box<dyn EventSource>> {
    Ok(match spec.kind {
        SourceKind::Ical => Box::new(IcalSource::new(spec.id.clone(), spec.require_url()?)),
        SourceKind::Rss => Box::new(RssSource::new(spec.id.clone(), spec.require_url()?)),
        SourceKind::Ticketmaster => Box::new(TicketmasterSource::from_spec(&spec)?),
        SourceKind::Html => {
            let adapter = spec.adapter.as_deref().with_context(|| {
                format!("source {:?} is kind=html but has no `adapter` set", spec.id)
            })?;
            let url = spec.require_url()?;
            match adapter {
                "bears" => Box::new(bears::BearsSource::new(spec.id.clone(), url)),
                other => bail!(
                    "source {:?}: unknown html adapter {other:?} (add it in sources/ and to build())",
                    spec.id
                ),
            }
        }
    })
}

/// Build every source from the spec file, in one shot. A single bad spec fails
/// the whole load (fail-fast on misconfiguration, same spirit as `Config`);
/// runtime *fetch* failures are handled per-source in the refresh loop instead.
pub fn build_all(path: &str) -> Result<Vec<Box<dyn EventSource>>> {
    load_specs(path)?.into_iter().map(build).collect()
}
