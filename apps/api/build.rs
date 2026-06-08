//! Build script — produce every served asset for the MASH HTML surface
//! (ADR-020 §O / §I).
//!
//! Three stages, sequenced; each runs after the previous so the
//! `static/` tree is fully populated before the Rust compile reads it.
//!
//! 1. **Tailwind compile.** `bun x @tailwindcss/cli` reads
//!    `styles/app.css` (the Tailwind directive entry-point) and scans
//!    `src/**/*.rs` for utility-class references in Maud templates,
//!    then emits the minified bundle to `static/app.css`.
//!
//! 2. **Vendored JS copy.** Distributed minified bundles for HTMX 2.x +
//!    extensions (sse, response-targets, preload) and Alpine.js 3.x are
//!    copied from `node_modules/` into `static/vendor/`. The browser
//!    loads each bundle via `<script src="/static/vendor/...">`. Pinned
//!    versions live in `package.json`; `bun.lock` guarantees the
//!    extracted bundle is byte-identical across machines.
//!
//! 3. **Content-hash + rename.** Each served file is hashed (SHA-256,
//!    first 8 hex chars) and renamed to `{stem}-{hash}.{ext}`. A Rust
//!    module is written to `OUT_DIR/asset_manifest.rs` carrying one
//!    `pub const` per asset (e.g. `APP_CSS`, `HTMX_JS`) whose value is
//!    the runtime URL — `/static/app-a1b2c3d4.css`. Templates resolve
//!    asset URLs through these constants, so a content change forces a
//!    URL change, and `Cache-Control: max-age=31536000, immutable`
//!    on `/static/*` is safe per ADR-020 §I.
//!
//! `static/` is gitignored — build artefacts reproduce from sources, not
//! from commits. The `embedded-static` cargo feature (rust-embed) lands
//! alongside the IBM Plex / `@theme` slice per ADR-020 §O.
//!
//! Bun is required at build time per ADR-020 §O (dev tooling, not
//! production runtime). If `apps/api/node_modules/` is absent the build
//! script runs `bun install --frozen-lockfile` first; afterwards
//! `bun x @tailwindcss/cli` and the vendor copy / hash steps resolve
//! from the local install. Network access is therefore required on the
//! first build after a fresh checkout but not on subsequent rebuilds.

use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use sha2::{Digest, Sha256};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Cargo re-runs build.rs when any of these change. Tracking `src/`
    // matches Tailwind v4's content-scanner scope so a template edit that
    // adds a class triggers a rebuild; without this the previous CSS would
    // be reused and the new class would render unstyled.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=styles/app.css");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=bun.lock");

    let static_dir = manifest_dir.join("static");

    // Cleanup before regenerate. Cargo only re-runs build.rs when one of
    // the rerun-if-changed paths changes, so this nuke is bounded — every
    // re-run produces a fresh `static/` tree from sources, with no stale
    // `{name}-{old-hash}.{ext}` files left lying around from a prior
    // content hash.
    let _ = std::fs::remove_dir_all(&static_dir);
    std::fs::create_dir_all(&static_dir).expect("create apps/api/static/");

    // First-time setup: install Tailwind CLI if Bun's node_modules is absent.
    // Subsequent builds skip this step — `bun x` resolves locally.
    if !manifest_dir.join("node_modules").exists() {
        eprintln!("build.rs: running `bun install --frozen-lockfile` (first-time setup)");
        let status = Command::new("bun")
            .current_dir(&manifest_dir)
            .args(["install", "--frozen-lockfile"])
            .status()
            .expect("invoke bun — install Bun 1.3+ per CONTRIBUTING.md (ADR-020 §O)");
        if !status.success() {
            eprintln!("build.rs: bun install failed ({status})");
            exit(1);
        }
    }

    let status = Command::new("bun")
        .current_dir(&manifest_dir)
        .args([
            "x",
            "@tailwindcss/cli",
            "-i",
            "./styles/app.css",
            "-o",
            "./static/app.css",
            "--minify",
        ])
        .status()
        .expect("invoke bun — install Bun 1.3+ per CONTRIBUTING.md (ADR-020 §O)");
    if !status.success() {
        eprintln!("build.rs: @tailwindcss/cli exited non-zero ({status})");
        exit(1);
    }

    // Vendored JS bundles for the MASH client layer (ADR-020 §F): HTMX
    // 2.x + extensions and Alpine.js 3.x. Each (source, target) is the
    // distributed minified bundle copied verbatim — no bundler, no
    // transformation, no concatenation. The version pins live in
    // `package.json` / `bun.lock`; the file paths under `dist/` are the
    // upstream convention for each package.
    let vendor_dir = static_dir.join("vendor");
    std::fs::create_dir_all(&vendor_dir).expect("create apps/api/static/vendor/");

    const VENDORED_JS: &[(&str, &str)] = &[
        // HTMX core + extensions. Extensions are activated per surface
        // via `hx-ext="..."` attributes; the script tag is loaded
        // unconditionally because the cost is small (~1-2KB per ext).
        (
            "node_modules/htmx.org/dist/htmx.min.js",
            "static/vendor/htmx.min.js",
        ),
        (
            "node_modules/htmx-ext-sse/dist/sse.min.js",
            "static/vendor/htmx-ext-sse.min.js",
        ),
        (
            "node_modules/htmx-ext-response-targets/dist/response-targets.min.js",
            "static/vendor/htmx-ext-response-targets.min.js",
        ),
        (
            "node_modules/htmx-ext-preload/dist/preload.min.js",
            "static/vendor/htmx-ext-preload.min.js",
        ),
        // Alpine.js, CSP-safe build (`@alpinejs/csp`). Drops the
        // `new Function()` evaluator that the standard build relies on
        // for inline `x-data` expressions, so strict CSP needs neither
        // `'unsafe-inline'` nor `'unsafe-eval'`. Trade-off: `x-data`,
        // `x-text`, `@click`, etc. accept registered component names
        // and property/method references — not arbitrary JS expressions.
        // Components are registered via `Alpine.data('name', () => ({...}))`
        // in an external script (so a CSP allow-list `script-src 'self'`
        // covers it). See ADR-020 §K and the Alpine CSP docs at
        // https://alpinejs.dev/advanced/csp.
        (
            "node_modules/@alpinejs/csp/dist/cdn.min.js",
            "static/vendor/alpine.min.js",
        ),
    ];

    for (src, dst) in VENDORED_JS {
        let src_path = manifest_dir.join(src);
        let dst_path = manifest_dir.join(dst);
        std::fs::copy(&src_path, &dst_path).unwrap_or_else(|e| {
            eprintln!(
                "build.rs: copy {} → {} failed: {}",
                src_path.display(),
                dst_path.display(),
                e,
            );
            exit(1);
        });
    }

    // Content-hash and rename. Every asset referenced from a Maud
    // template gets a hashed URL; templates pull the URL from a
    // constant emitted into `OUT_DIR/asset_manifest.rs`. Touching a
    // source file shifts the hash, shifts the URL, and a downstream
    // CDN's `immutable` cache naturally invalidates.
    //
    // The asset list lives here, not in src/, because `include!` only
    // works on paths Cargo's build script knows about; this is the
    // single source of truth for "what we serve".
    const ASSETS: &[Asset] = &[
        Asset {
            file_rel: "app.css",
            const_name: "APP_CSS",
        },
        Asset {
            file_rel: "vendor/htmx.min.js",
            const_name: "HTMX_JS",
        },
        Asset {
            file_rel: "vendor/htmx-ext-sse.min.js",
            const_name: "HTMX_EXT_SSE_JS",
        },
        Asset {
            file_rel: "vendor/htmx-ext-response-targets.min.js",
            const_name: "HTMX_EXT_RESPONSE_TARGETS_JS",
        },
        Asset {
            file_rel: "vendor/htmx-ext-preload.min.js",
            const_name: "HTMX_EXT_PRELOAD_JS",
        },
        Asset {
            file_rel: "vendor/alpine.min.js",
            const_name: "ALPINE_JS",
        },
    ];

    let mut manifest = String::from(
        "// Generated by build.rs — do not edit. Maps each served asset \
         to its content-hashed `/static/...` URL per ADR-020 §I.\n\n",
    );

    for asset in ASSETS {
        let src_path = static_dir.join(asset.file_rel);
        let bytes = std::fs::read(&src_path).unwrap_or_else(|e| {
            eprintln!("build.rs: read {} failed: {}", src_path.display(), e);
            exit(1);
        });
        let hash = short_hash(&bytes);
        let hashed_rel = inject_hash(asset.file_rel, &hash);
        let dst_path = static_dir.join(&hashed_rel);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("build.rs: mkdir {} failed: {}", parent.display(), e);
                exit(1);
            });
        }
        std::fs::rename(&src_path, &dst_path).unwrap_or_else(|e| {
            eprintln!(
                "build.rs: rename {} → {} failed: {}",
                src_path.display(),
                dst_path.display(),
                e,
            );
            exit(1);
        });

        writeln!(
            manifest,
            "pub const {}: &str = \"/static/{}\";",
            asset.const_name, hashed_rel,
        )
        .unwrap();
    }

    std::fs::write(out_dir.join("asset_manifest.rs"), manifest)
        .expect("write asset_manifest.rs to OUT_DIR");
}

/// One entry in the served-asset manifest. `file_rel` is relative to
/// `apps/api/static/`; `const_name` becomes the public Rust identifier
/// in the generated `assets` module.
struct Asset {
    file_rel: &'static str,
    const_name: &'static str,
}

/// First 8 hex chars of SHA-256(bytes). The 32-bit prefix is enough for
/// URL fingerprinting across the ~10 vendored assets — collision
/// probability is negligible at this scale, and the short suffix keeps
/// the URLs human-readable.
fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(8);
    for byte in &digest[..4] {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

/// Insert `hash` before the first `.` in the file name.
///
/// - `"app.css"` → `"app-a1b2c3d4.css"`
/// - `"vendor/htmx.min.js"` → `"vendor/htmx-a1b2c3d4.min.js"`
fn inject_hash(file_rel: &str, hash: &str) -> String {
    let path = Path::new(file_rel);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let file_name = path.file_name().expect("asset has a file name");
    let name_str = file_name.to_str().expect("asset name is utf-8");

    let (stem, ext) = match name_str.find('.') {
        Some(idx) => (&name_str[..idx], &name_str[idx..]),
        None => (name_str, ""),
    };
    let hashed_name = format!("{stem}-{hash}{ext}");

    match parent {
        Some(p) => format!("{}/{hashed_name}", p.display()),
        None => hashed_name,
    }
}
