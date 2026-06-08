//! Embedded static-asset serving for the self-host single-binary
//! distribution variant (ADR-020 §O, `--features embedded-static`).
//!
//! The default build serves `/static/*` from disk via
//! `tower_http::services::ServeDir` — appropriate for the hosted-production
//! deployment (CloudFront edge does the actual caching) and for local
//! development. The self-host artefact instead wants every byte the binary
//! emits to live inside the binary itself, so a single `docker run` or a
//! single `./flight-academy` invocation is fully self-contained.
//!
//! `rust-embed` reads the `apps/api/static/` directory at compile time
//! (the directory is populated by `build.rs` before the Rust compile starts)
//! and bakes each file's bytes into a `const fn get(path)` lookup. The
//! `mime-guess` feature gives us `Metadata::mimetype()` so the `Content-Type`
//! falls out of the file extension without a hand-maintained map. The
//! `debug-embed` feature forces compile-time embedding under `cargo test`
//! and debug builds — without it `rust-embed` would re-read the source
//! directory at runtime, which defeats the point of testing this code
//! path (we want tests to exercise the same lookup that ships).
//!
//! Cache-Control is intentionally NOT set here: the wrapping
//! `SetResponseHeaderLayer::if_not_present` in `lib.rs::build` emits
//! `public, max-age=31536000, immutable` for every `/static/*` response,
//! and the layer applies uniformly whether the inner service is `ServeDir`
//! or this module's handler. Keeping the cache policy in one place avoids
//! the two-paths-drifting failure mode.

use axum::{
    Router,
    extract::Path,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;

/// Compile-time embedding of every file under `apps/api/static/`. The
/// directory is gitignored but `build.rs` populates it before the Rust
/// compile starts (Tailwind CSS, vendored HTMX + Alpine bundles, every
/// asset content-hashed to `{stem}-{hash}.{ext}` per ADR-020 §I), so the
/// derive macro sees a fully-populated tree at expansion time.
#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

/// The static-asset service mounted at `/static/*` when the
/// `embedded-static` feature is on. Mirrors `ServeDir`'s
/// `Service<Request<Body>>` shape so `nest_service("/static", ...)` in
/// `lib.rs::build` can swap between them without further plumbing.
///
/// The catch-all route is `/{*path}`: `axum::Router::nest_service` strips
/// the `/static` prefix before routing, so a request for
/// `/static/vendor/htmx-a1b2c3d4.min.js` reaches this router as
/// `/vendor/htmx-a1b2c3d4.min.js` and the captured `path` is
/// `vendor/htmx-a1b2c3d4.min.js` — the same key `rust-embed` indexes by.
pub fn service() -> Router {
    Router::new().route("/{*path}", get(serve))
}

async fn serve(Path(path): Path<String>) -> Response {
    let Some(content) = StaticAssets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = content.metadata.mimetype();
    // `HeaderValue::from_str` only fails on out-of-band bytes; every MIME
    // returned by `mime_guess` is ASCII so the fallback is defensive only.
    let content_type = HeaderValue::from_str(mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    (
        [(header::CONTENT_TYPE, content_type)],
        content.data.into_owned(),
    )
        .into_response()
}
