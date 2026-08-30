//! Shared HTML chrome and the design system. Every page is
//! `layout(title, active, body)`, so per-job view modules render only their own
//! fragment and inherit the head, nav, Smeech chrome, and styling.
//!
//! **Design language — Smeech (boro / wabi-sabi).** The look is patched fabric:
//! an asymmetric bento grid of cards, each a cloth panel with a sashiko
//! running-stitch border and the odd corner patch, in a muted indigo/earth
//! palette, watched over by a black cat named Smeech. The whole system is:
//!   - **tokens** (`:root`) — the palette/type/space scale that is the portable
//!     artifact (the future SwiftUI theme mirrors these names);
//!   - **components** — one class per vocabulary entry (`.card` + variants,
//!     `.stat`, `.list`/`.row`, `.panel`/`.chart`, `.log`);
//!   - **decoration** — textures, patches, and cats, gated so data always wins.
//!
//! The one rule that keeps precise charts/calendars legible under all this:
//! *textured chrome wraps a calm content surface.* Fabric is only the frame; put
//! charts and grids inside a `.panel` and the boro never touches the data.
//!
//! Maud auto-escapes every interpolated value, which matters here: calendar
//! titles/locations/descriptions are *scraped from the web* (untrusted), so
//! escaping is the XSS defense. The one place that isn't automatic is a URL used
//! as an `href` — see [`safe_url`].

use maud::{html, Markup, PreEscaped, DOCTYPE};

/// Wrap a page fragment in the full document (head, Datastar, styles, the Smeech
/// shell + left nav). `active` is the nav key of the current page
/// (`"overview"`/`"calendar"`) so the sidebar shows where you are.
pub fn layout(title: &str, active: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Smeech" }
                // Datastar, vendored under /static so the LAN works offline.
                script type="module" src="/static/datastar.js" {}
                style { (PreEscaped(STYLES)) }
            }
            body {
                div.shell {
                    (nav(active))
                    main.container {
                        (body)
                    }
                }
            }
        }
    }
}

/// The left sidebar: brand, links (with the leather active-tab), and a status
/// stitch-patch. Small and hand-written — it's the one bit of chrome shared by
/// every page.
fn nav(active: &str) -> Markup {
    let link = |href: &str, key: &str, label: &str| {
        let class = if key == active {
            "nav__link is-active"
        } else {
            "nav__link"
        };
        html! { a class=(class) href=(href) { (label) } }
    };
    html! {
        aside.nav {
            div.brand {
                span.brand__name { "SMEECH" }
                span.brand__sub { "home server" }
                img.brand__cat src="/static/cat-peek.svg" alt="" aria-hidden="true";
            }
            nav.nav__links {
                (link("/", "overview", "Overview"))
                (link("/calendar", "calendar", "Calendar"))
            }
            div.nav__status {
                span.dot {}
                span { "All systems nominal" }
            }
        }
    }
}

/// The asymmetric card grid. Children are `.card`s; size them with the card size
/// axis classes. `grid-auto-flow: dense` backfills the gaps, so
/// the wabi-sabi asymmetry falls out of the sizing rather than manual placement.
pub fn bento(inner: Markup) -> Markup {
    html! { section.bento { (inner) } }
}

/// One number + its label — the `stat` vocabulary entry (fasting hours, weight,
/// counts). Mono + tabular so digits line up.
pub fn stat(value: &str, label: &str) -> Markup {
    html! {
        div.stat {
            span.stat__value { (value) }
            span.stat__label { (label) }
        }
    }
}

/// Return `url` only if it's a scheme we're willing to put in an `href`.
///
/// Scraped sources are untrusted, so a raw event URL could be
/// `javascript:...`/`data:...` — an XSS vector. Allowlist http(s) (define what
/// IS allowed, not what's forbidden); anything else renders as plain text, not a
/// link. Case-insensitive because schemes are.
pub fn safe_url(url: &str) -> Option<&str> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

const STYLES: &str = concat!(
    include_str!("../styles/shared.css"),
    "\n",
    include_str!("features/calendar/card.css")
);
