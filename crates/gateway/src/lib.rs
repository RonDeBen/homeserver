//! Gateway logic: the router, shared state, and the HTTP/SSE handlers.
//!
//! Two surfaces over one set of data, to prove the "one gateway, many
//! frontends" shape:
//!   - **JSON API** (`/api/*`) — what the future iOS app (and SDUI) will consume.
//!   - **Server-rendered HTML + Datastar** (`/`, `/calendar`) — the web
//!     dashboard, with live updates pushed over SSE straight off the bus.
//!
//! Adding a job's UI = add a module like [`calendar`] (a query, a view, a
//! handler or two) and a few routes below. That's the "just write another job"
//! ergonomic, carried up into the UI layer.

mod assets;
mod auth;
mod card;
mod card_lab;
mod features;
mod hub;
mod views;

use anyhow::Result;
use axum::{routing::get, Router};
use common::auth::AuthProvider;
use common::Context;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Shared, cheaply-cloneable handler state. Both fields are already
/// reference-counted, so `State` extraction just bumps a refcount.
#[derive(Clone)]
pub(crate) struct AppState {
    pub ctx: Arc<Context>,
    pub auth: Arc<dyn AuthProvider>,
}

/// Where the gateway binds. Not a connection secret like `DATABASE_URL`, so
/// unlike [`common::Config`] it has a sensible default rather than failing fast;
/// override with `GATEWAY_ADDR` (e.g. to bind loopback-only behind a proxy).
fn bind_addr() -> String {
    std::env::var("GATEWAY_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string())
}

/// Build the router, bind, and serve until a shutdown signal (reusing the same
/// `common::shutdown` every daemon uses, for a clean graceful stop).
pub async fn run(ctx: Context) -> Result<()> {
    let state = AppState {
        ctx: Arc::new(ctx),
        auth: common::auth::provider_from_env()?,
    };

    let addr = bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(%addr, "gateway listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(common::shutdown())
        .await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    // Everything a client touches sits behind the auth middleware...
    let protected = Router::new()
        .route("/", get(hub::page))
        .route("/calendar", get(features::calendar::page))
        .route("/calendar/view", get(features::calendar::view_fragment))
        .route("/calendar/month", get(features::calendar::month_fragment))
        .route("/calendar/events", get(features::calendar::events_sse))
        .route("/api/calendar", get(features::calendar::api_list))
        // Dev-only Card Lab: the sandbox for the visual language. Behind auth
        // like every human surface; linked from a small dev footer, not the nav.
        .route("/lab", get(card_lab::page))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    // ...except the liveness probe and vendored static assets, which must answer
    // before/without auth: the probe so a supervisor can health-check the
    // process, and /static so the login-less CSS/JS/decoration always loads.
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/static/{file}", get(assets::serve))
        .route("/static/boro/{*path}", get(assets::serve_boro))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
