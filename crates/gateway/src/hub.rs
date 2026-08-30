//! The Overview hub: the bento landing page. A "screen = sections of components"
//! — here, one `bento` of `card`s, each fed by a real read from Postgres. No job
//! is invented: cards render calendar, health (fasting/weight), and scheduled
//! jobs that actually exist; the system-metrics card is a marked `.future`
//! placeholder rather than faked numbers.
//!
//! Reads are plain SQL against the shared Postgres (the BFF pattern), and — like
//! the rest of the gateway — the hub doesn't depend on the producer crates
//! (`health`, `orchestrator`); it queries their tables directly to stay decoupled
//! from their heavier deps.

use crate::card::{Card, CardSize, CatFamily, Ornament, Recipe, Variation};
use crate::views::{self, bento, stat};
use crate::{features::calendar, AppState};
use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::Html};
use chrono::{DateTime, Datelike, Utc};
use common::Context;
use maud::{html, Markup};
use tracing::error;

/// `GET /` → the Overview hub.
pub async fn page(State(st): State<AppState>) -> Result<Html<String>, StatusCode> {
    let ctx = &st.ctx;
    // Each read is independent; a failure in one shouldn't blank the whole hub,
    // so pull them separately and let cards fall back to a friendly empty.
    let month = Utc::now().date_naive().with_day(1).unwrap();
    let events = crate::features::calendar::month_events(ctx, month)
        .await
        .unwrap_or_else(log_empty("hub calendar"));
    // let fast = open_fast(ctx).await.unwrap_or_else(log_empty("hub fast"));
    // let weight = latest_weight(ctx)
    //     .await
    //     .unwrap_or_else(log_empty("hub weight"));
    let jobs = next_jobs(ctx).await.unwrap_or_else(log_empty("hub jobs"));

    let body = html! {
        header.page-head {
            h1 { "Good morning" }
            p { "Smeech is keeping watch." }
        }
        (bento(html! {
        // (greeting_card())
        // (health_card(&fast, &weight))
        (jobs_card(&jobs))
            (calendar::render_hub_view(&events, month))
            (log_card())
            (metrics_placeholder())
        }))
    };
    Ok(Html(
        views::layout("Overview", "overview", body).into_string(),
    ))
}

/// The Smeech "good morning" hero — a Feature (linen) card with the resting cat
/// as its ornament. The lead speaks, so no label; Playful variation on the hero.
fn greeting_card() -> Markup {
    let content = html! {
        h2.card__lead { "Smeech is purring" }
        p.muted { "All systems are healthy. Enjoy your day." }
    };
    Card::new("hub-greeting", content)
        .recipe(Recipe::Feature)
        .size(CardSize::Wide)
        .ornament(Ornament::Smeech(CatFamily::Rest))
        .variation(Variation::Playful)
        .render_today()
}

/// Health teaser — current fast (elapsed vs target) + latest weigh-in. Feature
/// recipe (linen), the "Health" title now a cloth label.
fn health_card(fast: &Option<FastRow>, weight: &Option<WeightRow>) -> Markup {
    let content = html! {
        @if fast.is_none() && weight.is_none() {
            p.empty { "No health data yet — log a fast or weigh-in with hsctl." }
        } @else {
            div.stat-row {
                @if let Some(f) = fast {
                    (stat(&format!("{:.1}", f.elapsed_hours), fast_label(f)))
                }
                @if let Some(w) = weight {
                    (stat(&format!("{:.1}", w.weight_lbs), "lbs"))
                }
            }
        }
        a.card__link.card__foot href="/calendar" { "Health hub →" }
    };
    Card::new("hub-health", content)
        .recipe(Recipe::Feature)
        .label("Health")
        .render_today()
}

/// What the orchestrator is about to run — Healthy (olive).
fn jobs_card(jobs: &[JobRow]) -> Markup {
    let content = html! {
        @if jobs.is_empty() {
            p.empty { "Nothing scheduled." }
        } @else {
            ul.list {
                @for j in jobs {
                    li.row {
                        span.title { (j.name) }
                        span.when {
                            @match j.when {
                                Some(t) => (t.format("%a %b %-d, %H:%M").to_string()),
                                None => "—",
                            }
                        }
                    }
                }
            }
        }
    };
    Card::new("hub-jobs", content)
        .recipe(Recipe::Healthy)
        .label("Scheduled jobs")
        .render_today()
}

/// Smeech flavor — Quiet (charcoal), wide, with the walking cat ornament.
fn log_card() -> Markup {
    let content = html! {
        p.quote {
            "A watched server purrs."
            cite { "— Smeech" }
        }
    };
    Card::new("hub-log", content)
        .recipe(Recipe::Quiet)
        .size(CardSize::Wide)
        .label("Smeech says")
        .ornament(Ornament::Smeech(CatFamily::Active))
        .render_today()
}

/// Honest placeholder for host metrics — no such job exists yet, so it's marked
/// `.future` (dimmed) instead of showing invented numbers.
fn metrics_placeholder() -> Markup {
    let content = html! {
        p.muted { "Host metrics land here once a system-stats job is added." }
        span.badge { "soon" }
    };
    Card::new("hub-metrics", content)
        .recipe(Recipe::Active)
        .label("System load")
        .ornament(Ornament::None) // short placeholder — no room for Smeech
        .extra_class("future")
        .render_today()
}

// ── Reads ────────────────────────────────────────────────────────────────────

/// The open fast (if any), with elapsed hours computed at read time.
struct FastRow {
    elapsed_hours: f64,
    target_hours: Option<f64>,
}

/// State-aware label for the fasting stat, mirroring `health::FastStatus` words.
fn fast_label(f: &FastRow) -> &'static str {
    match f.target_hours {
        Some(t) if f.elapsed_hours >= t => "hrs · target met",
        Some(_) => "hrs fasting",
        None => "hrs · open fast",
    }
}

async fn open_fast(ctx: &Context) -> Result<Option<FastRow>> {
    let row: Option<(DateTime<Utc>, Option<f64>)> = sqlx::query_as(
        "SELECT started_at, target_hours FROM fasts
          WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(&ctx.db)
    .await?;
    Ok(row.map(|(started_at, target_hours)| FastRow {
        elapsed_hours: (Utc::now() - started_at).num_seconds() as f64 / 3600.0,
        target_hours,
    }))
}

struct WeightRow {
    weight_lbs: f64,
}

async fn latest_weight(ctx: &Context) -> Result<Option<WeightRow>> {
    let row: Option<(f64,)> =
        sqlx::query_as("SELECT weight_lbs FROM weigh_ins ORDER BY measured_at DESC LIMIT 1")
            .fetch_optional(&ctx.db)
            .await?;
    Ok(row.map(|(weight_lbs,)| WeightRow { weight_lbs }))
}

struct JobRow {
    name: String,
    when: Option<DateTime<Utc>>,
}

/// The next few things the orchestrator will fire: enabled recurring schedules by
/// `next_fire`, unioned with one-shot timers by `fire_at`, soonest first.
async fn next_jobs(ctx: &Context) -> Result<Vec<JobRow>> {
    let rows: Vec<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT name, next_fire AS when FROM schedules WHERE enabled AND next_fire IS NOT NULL
         UNION ALL
         SELECT subject AS name, fire_at AS when FROM timers
         ORDER BY when ASC NULLS LAST
         LIMIT 5",
    )
    .fetch_all(&ctx.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, when)| JobRow { name, when })
        .collect())
}

/// Log the error and yield an empty value, so one failed hub read degrades to a
/// friendly empty card instead of a 500 for the whole page.
fn log_empty<T: Default>(context: &'static str) -> impl Fn(anyhow::Error) -> T {
    move |e| {
        error!(error = %e, "{context}");
        T::default()
    }
}
