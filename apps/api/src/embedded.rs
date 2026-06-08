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
//! `Accept-Encoding` content negotiation matches the `ServeDir` path: for
//! every requested asset we look first for an embedded `{path}.zst`
//! sibling, then `{path}.br`, then `{path}.gz`, then the raw bytes.
//! Whichever variant the client supports wins; `Content-Encoding` +
//! `Vary: Accept-Encoding` tell upstream caches to key on the encoding
//! so a zstd-supporting client can't be served a gzip-cached response
//! (or vice-versa). Server priority is zstd > br > gzip > identity
//! (ADR-020 §I): zstd at the top because Chrome 123+ / Firefox 126+
//! advertise it and it matches brotli q11 on size with ~2× faster
//! decompress; brotli second for the broader installed base; gzip the
//! universal fallback; identity for the rare clients that refuse all
//! encodings.
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
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;

/// Compile-time embedding of every file under `apps/api/static/`. The
/// directory is gitignored but `build.rs` populates it before the Rust
/// compile starts (Tailwind CSS, vendored HTMX + Alpine bundles, every
/// asset content-hashed to `{stem}-{hash}.{ext}` per ADR-020 §I; `.br` +
/// `.gz` precompressed siblings for every text asset), so the derive
/// macro sees a fully-populated tree at expansion time.
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

async fn serve(headers: HeaderMap, Path(path): Path<String>) -> Response {
    // The identity bytes must exist for the request to be valid — even
    // when we serve a precompressed sibling, the MIME comes from the
    // original file's extension (`text/css`, not `application/brotli`).
    if StaticAssets::get(&path).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let accept_encoding = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Probe precompressed siblings in server-preference order. The
    // `Vary: Accept-Encoding` header is the load-bearing piece for any
    // downstream cache (browser, CDN, corporate proxy) — without it
    // they'd return a zstd body to a brotli-only client on the next
    // request. The handler emits it on every response, even the
    // identity fallback, because the choice of encoding is a function
    // of `Accept-Encoding` in EVERY case.
    let (encoded_path, encoding) = pick_encoding(&path, accept_encoding);

    // Content-Type comes from the ORIGINAL path's extension, not the
    // encoded sibling's — `text/css` for `/static/app.css.br`, not
    // `application/brotli`. `Metadata::mimetype()` reads only the
    // trailing extension on the *original* file name; we feed it
    // `path`, not `encoded_path`.
    let identity = StaticAssets::get(&path).expect("identity bytes presence checked above");
    let content_type = HeaderValue::from_str(identity.metadata.mimetype())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));

    let bytes = StaticAssets::get(&encoded_path)
        .expect("encoded variant presence checked above")
        .data
        .into_owned();

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    if let Some(enc) = encoding {
        response = response.header(
            header::CONTENT_ENCODING,
            HeaderValue::from_static(match enc {
                "zstd" => "zstd",
                "br" => "br",
                "gzip" => "gzip",
                _ => unreachable!("encoding tagged with one of zstd/br/gzip"),
            }),
        );
    }
    response
        .body(bytes.into())
        .expect("constructed response is well-formed")
}

/// Walk server-preference order (`zstd > br > gzip > identity`) against
/// the client's `Accept-Encoding` advertisements. Returns the embedded
/// asset key to look up and the encoding label (or `None` for identity).
///
/// Probes a precompressed sibling only if (a) the client advertises the
/// coding with a non-zero q-value AND (b) the sibling file exists in
/// the embed. The first hit wins — keeps the priority order honest and
/// avoids a partial-coverage fallback that would silently re-rank.
fn pick_encoding(path: &str, accept_encoding: &str) -> (String, Option<&'static str>) {
    for (coding, suffix) in [("zstd", "zst"), ("br", "br"), ("gzip", "gz")] {
        if accepts(accept_encoding, coding) {
            let sibling = format!("{path}.{suffix}");
            if StaticAssets::get(&sibling).is_some() {
                return (sibling, Some(coding));
            }
        }
    }
    (path.to_string(), None)
}

/// Does the `Accept-Encoding` header advertise `encoding` with non-zero
/// `q`-value? Implements the subset of RFC 7231 §5.3.4 we actually need:
/// a comma-separated list of codings, each optionally followed by
/// `;q=<value>`. Default qvalue is 1.0; explicit `q=0` means refuse.
///
/// We don't honour `*` (wildcard) because no real-world browser sends it
/// for compression encodings; and we don't rank multiple supported
/// encodings by qvalue because tower-http doesn't either — server
/// preference order (br > gzip > identity) wins.
fn accepts(accept_encoding: &str, encoding: &str) -> bool {
    accept_encoding.split(',').any(|part| {
        let mut sub = part.split(';');
        let coding = sub.next().unwrap_or("").trim();
        if !coding.eq_ignore_ascii_case(encoding) {
            return false;
        }
        // Walk qvalue params. Any `q=0` (with optional decimal zeros)
        // means the client explicitly refuses this encoding.
        !sub.any(|p| {
            let p = p.trim();
            matches!(p, "q=0" | "q=0.0" | "q=0.00" | "q=0.000")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::accepts;

    #[test]
    fn accepts_explicit_encoding() {
        assert!(accepts("br", "br"));
        assert!(accepts("gzip", "gzip"));
        assert!(accepts("br, gzip", "br"));
        assert!(accepts("br, gzip", "gzip"));
        assert!(accepts("gzip, deflate, br", "br"));
    }

    #[test]
    fn rejects_missing_encoding() {
        assert!(!accepts("gzip", "br"));
        assert!(!accepts("", "br"));
        assert!(!accepts("identity", "br"));
    }

    #[test]
    fn honours_q_zero_refusal() {
        assert!(!accepts("br;q=0", "br"));
        assert!(!accepts("br;q=0.0", "br"));
        assert!(!accepts("br;q=0.000, gzip", "br"));
        assert!(accepts("br;q=0.000, gzip", "gzip"));
    }

    #[test]
    fn case_insensitive_coding_match() {
        assert!(accepts("BR", "br"));
        assert!(accepts("GZip", "gzip"));
    }
}
