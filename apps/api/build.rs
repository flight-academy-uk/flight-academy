//! Build script — produce every served asset for the MASH HTML surface
//! (ADR-020 §O).
//!
//! Two stages, sequenced; the second runs after Tailwind so the static/
//! tree is fully populated before the Rust compile reads it.
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
//! `static/` is gitignored — build artefacts reproduce from sources, not
//! from commits. Content-hashed asset URLs and the `embedded-static`
//! cargo feature (rust-embed) land in the next slices per ADR-020 §O.
//!
//! Bun is required at build time per ADR-020 §O (dev tooling, not
//! production runtime). If `apps/api/node_modules/` is absent the build
//! script runs `bun install --frozen-lockfile` first; afterwards
//! `bun x @tailwindcss/cli` and the vendor copy step resolve from the
//! local install. Network access is therefore required on the first
//! build after a fresh checkout but not on subsequent rebuilds.

use std::env;
use std::path::PathBuf;
use std::process::{Command, exit};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

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
}
