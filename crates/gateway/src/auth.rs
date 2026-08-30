//! The axum-aware half of auth: one middleware that runs the configured
//! [`common::auth::AuthProvider`] on every protected request. This is the only
//! place the gateway touches the auth machinery — swapping the provider (env
//! `AUTH_MODE`) changes behavior without touching this file or any handler.

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::warn;

/// Reject unauthenticated requests with a bare `401`. We log the reason
/// server-side but return a generic body to the client — failing closed and
/// leaking nothing about *why* a credential was refused.
pub async fn require_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    match state.auth.authenticate(request.headers()) {
        Ok(_identity) => next.run(request).await,
        Err(reason) => {
            warn!(?reason, path = %request.uri().path(), "rejected unauthenticated request");
            (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
        }
    }
}
