//! Content-hashed asset URLs (ADR-020 §I).
//!
//! Each `pub const` here is the runtime URL of one served asset — e.g.
//! `APP_CSS = "/static/app-a1b2c3d4.css"`. The constants are generated
//! by `build.rs` and written to `$OUT_DIR/asset_manifest.rs`; this
//! module is the public consumption surface for Maud templates and any
//! other code that needs to reference an asset.
//!
//! The hash suffix changes when the asset's bytes change, which is what
//! makes `Cache-Control: public, max-age=31536000, immutable` safe on
//! `/static/*` (ADR-020 §I): the URL itself is content-addressed, so a
//! stale cached entry is always for the right version.

include!(concat!(env!("OUT_DIR"), "/asset_manifest.rs"));
