//! Static assets, vendored into the binary so the dashboard works fully offline
//! on the LAN (no CDN, no external fonts). Everything here is decoration or the
//! Datastar runtime — never data — so it's served unauthenticated alongside
//! `/healthz` and cached hard by the browser.
//!
//! Assets are `include_*!`d from `crates/gateway/assets/` at compile time, so the
//! deploy stays a single binary (matches the `Dockerfile`); there's no directory
//! to ship or `ServeDir` to wire. The SVGs are hand-authored (cat sprites, boro
//! patches, fabric grain) — vector, so they theme and scale for free.

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};
use include_dir::{include_dir, Dir};

/// The vendored boro asset tree (fabric textures, cloth labels, repair patches,
/// frays, sashiko, badges, Smeech sprites), embedded at compile time so the
/// deploy stays a single binary. Produced by `scripts/boro-assets.sh`.
static BORO: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/boro");

/// `GET /static/boro/{*path}` → an embedded boro asset by relative path. Path is
/// matched against the embedded tree (not the filesystem), so there's no
/// traversal surface: an unknown path is a plain 404.
pub async fn serve_boro(Path(path): Path<String>) -> impl IntoResponse {
    match BORO.get_file(&path) {
        Some(file) => {
            let ct = match path.rsplit('.').next() {
                Some("webp") => "image/webp",
                Some("png") => "image/png",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            (
                [
                    (header::CONTENT_TYPE, ct),
                    // `no-cache` = the browser revalidates every load, so re-processed
                    // assets (stable filenames) never serve stale while we iterate.
                    // Switch to hashed filenames + `immutable` for a public deploy.
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                file.contents(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Datastar runtime, pinned to the version the handlers target. Vendored from the
/// CDN once; bump deliberately alongside the `datastar` crate dep.
const DATASTAR_JS: &str = include_str!("../assets/datastar.js");

// const NOISE_SVG: &str = include_str!("../assets/noise.svg");
// const PATCH_SASHIKO_SVG: &str = include_str!("../assets/patch-sashiko.svg");
// const PATCH_CALICO_SVG: &str = include_str!("../assets/patch-calico.svg");
// const CAT_PEEK_SVG: &str = include_str!("../assets/cat-peek.svg");
// const CAT_SLEEPING_SVG: &str = include_str!("../assets/cat-sleeping.svg");
// const CAT_WALK_SVG: &str = include_str!("../assets/cat-walk.svg");
//
/// `GET /static/{file}` → a vendored asset by exact filename. An allowlist match
/// (not a filesystem lookup) so there's no path-traversal surface: an unknown
/// name is a plain 404, nothing on disk is reachable.
pub async fn serve(Path(file): Path<String>) -> impl IntoResponse {
    // let svg = "image/svg+xml; charset=utf-8";
    let (content_type, body) = match file.as_str() {
        "datastar.js" => ("text/javascript; charset=utf-8", DATASTAR_JS),
        // "noise.svg" => (svg, NOISE_SVG),
        // "patch-sashiko.svg" => (svg, PATCH_SASHIKO_SVG),
        // "patch-calico.svg" => (svg, PATCH_CALICO_SVG),
        // "cat-peek.svg" => (svg, CAT_PEEK_SVG),
        // "cat-sleeping.svg" => (svg, CAT_SLEEPING_SVG),
        // "cat-walk.svg" => (svg, CAT_WALK_SVG),
        _ => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            // Immutable: every asset is either pinned (Datastar) or hand-edited
            // with the source, so a long cache is safe and keeps the LAN snappy.
            (header::CACHE_CONTROL, "public, max-age=604800, immutable"),
        ],
        body,
    )
        .into_response()
}
