//! Authentication, abstracted so no HTTP job hard-codes *how* a request is
//! trusted — only *whether* it is, and *as whom*.
//!
//! The whole system is one household, self-hosted. The default deployment puts
//! the gateway on a private network (Tailscale / LAN), where reachability *is*
//! the auth boundary — so the default provider trusts every request. The seam
//! exists so that "expose this to the public internet" is a config change
//! ([`BearerToken`], or a future session/passkey provider), not a rewrite of
//! every handler: handlers depend on [`AuthProvider`], never on Tailscale.
//!
//! An HTTP job turns a request into an [`Identity`] (or a rejection) by calling
//! its configured provider; the axum middleware that does this lives in the
//! job (it's the only axum-aware part), keeping this module framework-free.

use anyhow::{bail, Context as _, Result};
use http::header::AUTHORIZATION;
use http::HeaderMap;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Who a request is, once authenticated. Deliberately minimal for a single-user
/// system — a name for logging/attribution. Add fields (scopes, roles) if a job
/// ever needs finer-grained authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
}

/// Why a request was rejected. Callers map this to a generic `401` — we never
/// leak *which* check failed to the client (that only helps an attacker probe);
/// the distinction is for server-side logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// No credential presented at all.
    Missing,
    /// A credential was presented but didn't check out.
    Invalid,
}

/// Turns an incoming request's headers into an [`Identity`] or an [`AuthError`].
///
/// Object-safe on purpose: jobs hold an `Arc<dyn AuthProvider>` chosen at
/// startup, so the concrete scheme is swappable without touching call sites.
pub trait AuthProvider: Send + Sync + 'static {
    fn authenticate(&self, headers: &HeaderMap) -> Result<Identity, AuthError>;
}

/// Trust every request. For deployment behind a private network boundary
/// (Tailscale tailnet / trusted LAN) where the network *is* the perimeter.
///
/// This is a deliberate, documented choice, not an oversight: on a tailnet the
/// request never traversed the public internet, so re-authenticating at the app
/// layer buys nothing. Do **not** use this when the gateway is publicly
/// reachable — switch to [`BearerToken`] (or a real session provider) there.
pub struct TrustedNetwork;

impl AuthProvider for TrustedNetwork {
    fn authenticate(&self, _headers: &HeaderMap) -> Result<Identity, AuthError> {
        Ok(Identity {
            name: "trusted-network".to_string(),
        })
    }
}

/// Require a static `Authorization: Bearer <token>` matching a shared secret.
///
/// The belt-and-suspenders option for public exposure. The token comes from the
/// environment (`AUTH_TOKEN`), never source (Rule A). Assumes transport is
/// encrypted (TLS / tailnet) — a bearer token on plain HTTP is sniffable, so
/// front this with TLS if it leaves a trusted network.
pub struct BearerToken {
    token: String,
}

impl BearerToken {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

impl AuthProvider for BearerToken {
    fn authenticate(&self, headers: &HeaderMap) -> Result<Identity, AuthError> {
        let header = headers.get(AUTHORIZATION).ok_or(AuthError::Missing)?;
        // A non-ASCII/garbage header is an invalid credential, not a server error.
        let value = header.to_str().map_err(|_| AuthError::Invalid)?;
        let presented = value.strip_prefix("Bearer ").ok_or(AuthError::Invalid)?;

        // Constant-time compare: a byte-by-byte `==` would leak how many leading
        // bytes matched via timing, letting an attacker recover the token one
        // byte at a time. `ct_eq` also returns false (in constant time) on a
        // length mismatch, so it's safe on differing lengths.
        if bool::from(presented.as_bytes().ct_eq(self.token.as_bytes())) {
            Ok(Identity {
                name: "bearer-token".to_string(),
            })
        } else {
            Err(AuthError::Invalid)
        }
    }
}

/// Build the provider from the environment, fail-fast on misconfiguration
/// (mirrors [`crate::Config`]).
///
/// `AUTH_MODE`:
///   - unset / `trusted` → [`TrustedNetwork`] (the default, private-network deploy)
///   - `token`           → [`BearerToken`], requiring `AUTH_TOKEN` to be set
///
/// An unknown mode, or `token` without `AUTH_TOKEN`, is a misconfiguration we
/// refuse to start with — failing closed rather than silently trusting everyone.
pub fn provider_from_env() -> Result<Arc<dyn AuthProvider>> {
    let mode = std::env::var("AUTH_MODE").unwrap_or_else(|_| "trusted".to_string());
    match mode.as_str() {
        "trusted" => Ok(Arc::new(TrustedNetwork)),
        "token" => {
            let token = std::env::var("AUTH_TOKEN")
                .context("AUTH_MODE=token requires AUTH_TOKEN to be set")?;
            if token.is_empty() {
                bail!("AUTH_TOKEN is set but empty");
            }
            Ok(Arc::new(BearerToken::new(token)))
        }
        other => bail!("unknown AUTH_MODE {other:?} (expected 'trusted' or 'token')"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bearer(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, value.parse().unwrap());
        h
    }

    #[test]
    fn trusted_network_accepts_anything() {
        assert!(TrustedNetwork.authenticate(&HeaderMap::new()).is_ok());
    }

    #[test]
    fn bearer_token_matches_correct_secret() {
        let p = BearerToken::new("s3cret".to_string());
        assert!(p.authenticate(&bearer("Bearer s3cret")).is_ok());
    }

    #[test]
    fn bearer_token_rejects_wrong_secret_and_missing_header() {
        let p = BearerToken::new("s3cret".to_string());
        assert_eq!(
            p.authenticate(&bearer("Bearer nope")),
            Err(AuthError::Invalid)
        );
        assert_eq!(p.authenticate(&bearer("s3cret")), Err(AuthError::Invalid)); // no "Bearer " prefix
        assert_eq!(p.authenticate(&HeaderMap::new()), Err(AuthError::Missing));
    }
}
