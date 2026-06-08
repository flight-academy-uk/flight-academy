//! Build script — compile Tailwind v4 CSS for the MASH HTML surface (ADR-020 §O).
//!
//! Runs `bun x @tailwindcss/cli` once per Cargo build. Inputs:
//!
//!   - `styles/app.css` — Tailwind directive entry-point.
//!   - `src/**/*.rs` — Tailwind v4's content scanner extracts utility-class
//!     references from Maud templates compiled into the binary.
//!
//! Output:
//!
//!   - `static/app.css` — minified bundle served by the `/static/*`
//!     `tower-http::services::ServeDir` route. The `static/` directory is
//!     gitignored; build artefacts produce themselves at build time, not at
//!     commit time. Content-hashed asset URLs + the `embedded-static` cargo
//!     feature (rust-embed) land in the MASH foundations PR B (ADR-020 §O).
//!
//! Bun is required at build time per ADR-020 §O (dev tooling, not production
//! runtime). If `apps/api/node_modules/` is absent the build script runs
//! `bun install --frozen-lockfile` first; afterwards `bun x @tailwindcss/cli`
//! resolves from the local install. Network access is therefore required on
//! the first build after a fresh checkout but not on subsequent rebuilds.

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
}
