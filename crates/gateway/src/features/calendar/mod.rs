//! Calendar surface: read upcoming events from Postgres and expose them three
//! ways — JSON for programmatic clients, a server-rendered HTML page for the
//! web dashboard, and an SSE stream that live-patches that page whenever the
//! calendar changes.
//!
//! The live-update trigger is the bus: the calendar job emits `calendar.updated`
//! after every refresh/submit. We don't need its *payload* — the message is
//! just a "something changed" signal; we re-read Postgres (the source of truth)
//! and push a fresh fragment. That keeps the gateway decoupled from the
//! calendar crate's types while staying correct.

mod card;

pub(crate) use card::render_hub_view;

use crate::views;
use crate::AppState;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive},
        Html, Sse,
    },
    Json,
};
use chrono::{DateTime, Datelike, Months, NaiveDate, Utc};
use common::Context;
use datastar::prelude::PatchElements;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tracing::error;

/// Bus subject the calendar job announces changes on. Owned by the calendar
/// crate; duplicated here as a literal rather than pulling that crate's heavy
/// scraping deps into the gateway just for one `const`. If a lightweight shared
/// events crate ever appears, import the typed subject from it instead.
const CALENDAR_UPDATED: &str = "calendar.updated";

/// How many upcoming events any surface returns. Plenty for one household; keeps
/// an unbounded table from becoming an unbounded response/DOM.
const UPCOMING_LIMIT: i64 = 100;

/// A calendar event as the API/UI sees it — a serializable read-model, distinct
/// from the calendar crate's ingest struct. Only fields a client needs.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CalendarEventDto {
    pub source: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarQuery {
    /// Selected month in `YYYY-MM` form. Invalid or absent values fall back to
    /// the current UTC month so a malformed link never produces a 400 page.
    month: Option<String>,
    /// Rendering context used when a reusable calendar view is navigated from
    /// the hub rather than the full calendar page.
    view: Option<String>,
}

fn selected_month(query: &CalendarQuery) -> NaiveDate {
    query
        .month
        .as_deref()
        .and_then(|month| NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok())
        .unwrap_or_else(|| Utc::now().date_naive().with_day(1).unwrap())
}

pub(crate) async fn month_events(ctx: &Context, month: NaiveDate) -> Result<Vec<CalendarEventDto>> {
    let next_month = month.checked_add_months(Months::new(1)).unwrap();
    let start = month.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = next_month.and_hms_opt(0, 0, 0).unwrap().and_utc();
    Ok(sqlx::query_as::<_, CalendarEventDto>(
        "SELECT source, title, starts_at, ends_at, location, url, description
           FROM calendar_events
          WHERE starts_at >= $1 AND starts_at < $2
          ORDER BY starts_at ASC
          LIMIT $3",
    )
    .bind(start)
    .bind(end)
    .bind(UPCOMING_LIMIT)
    .fetch_all(&ctx.db)
    .await?)
}

/// Upcoming events, soonest first. Static SQL, no user input in the query —
/// still parameterized-by-construction (bound `LIMIT`), nothing interpolated.
/// `pub(crate)` so the hub can reuse it for its calendar teaser card.
pub(crate) async fn upcoming(ctx: &Context) -> Result<Vec<CalendarEventDto>> {
    let rows = sqlx::query_as::<_, CalendarEventDto>(
        "SELECT source, title, starts_at, ends_at, location, url, description
           FROM calendar_events
          WHERE starts_at >= now()
          ORDER BY starts_at ASC
          LIMIT $1",
    )
    .bind(UPCOMING_LIMIT)
    .fetch_all(&ctx.db)
    .await?;
    Ok(rows)
}

/// `GET /api/calendar` → JSON list. What the iOS app / SDUI layer will consume.
pub async fn api_list(
    State(st): State<AppState>,
) -> Result<Json<Vec<CalendarEventDto>>, StatusCode> {
    let events = upcoming(&st.ctx)
        .await
        .map_err(internal("api calendar query"))?;
    Ok(Json(events))
}

/// `GET /calendar` → the full HTML page. Renders both cards server-side (works
/// with JS off), then `#calendar-view` opens the SSE stream for its month.
pub async fn page(
    State(st): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CalendarQuery>,
) -> Result<Html<String>, StatusCode> {
    let month = selected_month(&query);
    let events = month_events(&st.ctx, month)
        .await
        .map_err(internal("calendar page query"))?;
    let body = card::render_view(&events, month);
    Ok(Html(
        views::layout("Calendar", "calendar", body).into_string(),
    ))
}

/// `GET /calendar/view` → both coordinated card fragments for a selected month.
pub async fn view_fragment(
    State(st): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CalendarQuery>,
) -> Result<Html<String>, StatusCode> {
    let month = selected_month(&query);
    let events = month_events(&st.ctx, month)
        .await
        .map_err(internal("calendar view query"))?;
    let view = match query.view.as_deref() {
        Some("hub") => card::render_hub_view(&events, month),
        _ => card::render_view(&events, month),
    };
    Ok(Html(view.into_string()))
}

/// `GET /calendar/month` → one card-sized month fragment, retained for simple
/// clients that only need the month surface.
pub async fn month_fragment(
    State(st): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CalendarQuery>,
) -> Result<Html<String>, StatusCode> {
    let month = selected_month(&query);
    let events = month_events(&st.ctx, month)
        .await
        .map_err(internal("calendar month query"))?;
    Ok(Html(card::render_month_card(&events, month).into_string()))
}

/// `GET /calendar/events` → SSE. Pushes the current fragment on connect, then a
/// fresh one every time `calendar.updated` fires on the bus. Each client gets
/// its own subscription; fine at household scale.
pub async fn events_sse(
    State(st): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CalendarQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let ctx = st.ctx.clone();
    let month = selected_month(&query);
    let mut updates = ctx
        .bus
        .subscribe(CALENDAR_UPDATED.to_string())
        .await
        .map_err(|e| {
            error!(error = %e, "failed to subscribe to calendar.updated");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let stream = async_stream::stream! {
        // Render the current state immediately so a freshly-connected client
        // isn't stuck on the page's initial server-rendered snapshot.
        for event in render_patches(&ctx, month, query.view.as_deref() == Some("hub")).await { yield Ok(event); }
        // Then re-render on every change announcement.
        while updates.next().await.is_some() {
            for event in render_patches(&ctx, month, query.view.as_deref() == Some("hub")).await { yield Ok(event); }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Query + render the events fragment as a Datastar element patch, or `None` if
/// the query failed (logged; a transient DB hiccup shouldn't kill the stream —
/// the next announcement will try again).
async fn render_patches(ctx: &Context, month: NaiveDate, hub: bool) -> Vec<Event> {
    match month_events(ctx, month).await {
        Ok(events) => vec![
            PatchElements::new(if hub {
                card::render_hub_month_card(&events, month).into_string()
            } else {
                card::render_month_card(&events, month).into_string()
            })
            .write_as_axum_sse_event(),
            PatchElements::new(card::render_events_card(&events, month).into_string())
                .write_as_axum_sse_event(),
        ],
        Err(e) => {
            error!(error = %e, "calendar re-render failed");
            Vec::new()
        }
    }
}

/// Map an internal error to a bare `500`, logging the detail server-side. The
/// client never sees the underlying message (no SQL/paths leaked); `context`
/// tags the log so we know which query failed.
fn internal(context: &'static str) -> impl Fn(anyhow::Error) -> StatusCode {
    move |e| {
        error!(error = %e, "{context}");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
