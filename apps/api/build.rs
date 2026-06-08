//! Build script — produce every served asset for the MASH HTML surface
//! (ADR-020 §O / §I / §E).
//!
//! Four stages, sequenced; each runs after the previous so the `static/`
//! tree is fully populated before the Rust compile reads it.
//!
//! 1. **Tailwind compile.** `bun x @tailwindcss/cli` reads
//!    `styles/app.css` (the Tailwind directive entry-point + design-token
//!    `@theme` mapping from `apps/web-ui/tokens/tokens.css`) and scans
//!    `src/**/*.rs` for utility-class references in Maud templates,
//!    then emits the minified bundle to `static/app.css`.
//!
//! 2. **Vendored JS + woff2 copy.** Distributed minified bundles for
//!    HTMX 2.x + extensions and Alpine.js 3.x land in `static/vendor/`;
//!    IBM Plex Sans (regular + bold) and IBM Plex Mono (regular) woff2
//!    files, subsetted to Latin + Latin Extended, land in `static/fonts/`.
//!    Pinned versions live in `package.json`; `bun.lock` keeps every
//!    extracted byte identical across machines.
//!
//! 3. **Content-hash + rename + emit `fonts.css`.** Each served file is
//!    hashed (SHA-256, first 8 hex chars) and renamed to
//!    `{stem}-{hash}.{ext}`. Once the woff2 files have their final
//!    hashed URLs, `fonts.css` is emitted with `@font-face` declarations
//!    that point at them — then `fonts.css` itself is hashed too. A
//!    Rust module is written to `OUT_DIR/asset_manifest.rs` carrying one
//!    `pub const` per asset (e.g. `APP_CSS`, `FONTS_CSS`, `HTMX_JS`,
//!    `FONT_PLEX_SANS_LATIN_400`) whose value is the runtime URL.
//!    Templates resolve asset URLs through these constants, so a content
//!    change forces a URL change, and
//!    `Cache-Control: max-age=31536000, immutable` on `/static/*` is
//!    safe per ADR-020 §I.
//!
//! `static/` is gitignored — build artefacts reproduce from sources, not
//! from commits. The `embedded-static` cargo feature wraps the entire
//! tree into the binary at compile time per ADR-020 §O.
//!
//! Bun is required at build time per ADR-020 §O (dev tooling, not
//! production runtime). If `apps/api/node_modules/` is absent the build
//! script runs `bun install --frozen-lockfile` first; afterwards
//! `bun x @tailwindcss/cli` and the vendor copy / hash steps resolve
//! from the local install. Network access is therefore required on the
//! first build after a fresh checkout but not on subsequent rebuilds.

use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::io::Write as _;
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
    // Token JSON drives the `@theme` mapping in `styles/app.css`; a token
    // value change must shift the compiled `app.css` hash so cached
    // stylesheets invalidate. Tailwind scans by file ref, not by token
    // resolution, so we wire the dependency here explicitly.
    println!("cargo:rerun-if-changed=../web-ui/tokens/tokens.css");

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
        copy_vendor_file(&manifest_dir, src, dst);
    }

    // Vendored IBM Plex woff2 typography (ADR-020 §O step 4). Latin +
    // Latin Extended subsets cover every UK CAA / EASA contributor
    // surface plus the Polish / German / French accented characters seen
    // across the EU operator population. Other subsets (Cyrillic,
    // Pi/IPA, etc.) ship with the @fontsource package but are not
    // vendored — they would inflate the self-host binary under
    // `--features embedded-static` for tiny browser benefit. Three
    // weights cover the type ramp: Sans 400 + 700 for body + display,
    // Mono 400 for tabular / code. The hashed @font-face declarations
    // are emitted into `static/fonts.css` after Phase 3 below — the
    // file ordering means the @font-face URLs always match the on-disk
    // woff2 names.
    let fonts_dir = static_dir.join("fonts");
    std::fs::create_dir_all(&fonts_dir).expect("create apps/api/static/fonts/");

    const VENDORED_FONTS: &[(&str, &str)] = &[
        // IBM Plex Sans, regular weight, Latin + Latin Ext subsets.
        (
            "node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-400-normal.woff2",
            "static/fonts/ibm-plex-sans-latin-400.woff2",
        ),
        (
            "node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-ext-400-normal.woff2",
            "static/fonts/ibm-plex-sans-latin-ext-400.woff2",
        ),
        // IBM Plex Sans, bold weight, Latin + Latin Ext.
        (
            "node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-700-normal.woff2",
            "static/fonts/ibm-plex-sans-latin-700.woff2",
        ),
        (
            "node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-ext-700-normal.woff2",
            "static/fonts/ibm-plex-sans-latin-ext-700.woff2",
        ),
        // IBM Plex Mono, regular weight only — tabular / code surfaces
        // don't need bold within the type ramp v1; revisit if reviewed
        // dashboards demand mono bold for emphasis.
        (
            "node_modules/@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-400-normal.woff2",
            "static/fonts/ibm-plex-mono-latin-400.woff2",
        ),
        (
            "node_modules/@fontsource/ibm-plex-mono/files/ibm-plex-mono-latin-ext-400-normal.woff2",
            "static/fonts/ibm-plex-mono-latin-ext-400.woff2",
        ),
    ];

    for (src, dst) in VENDORED_FONTS {
        copy_vendor_file(&manifest_dir, src, dst);
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
    // Base assets — everything that exists on disk before `fonts.css` is
    // emitted. The order matters: fonts must be hashed before `fonts.css`
    // is written so the @font-face URLs inside it match the on-disk
    // names. After this list is processed, `fonts.css` is generated from
    // the captured URL map, then hashed as a final pass.
    const BASE_ASSETS: &[Asset] = &[
        Asset {
            file_rel: "app.css",
            const_name: "APP_CSS",
            precompress: true,
        },
        Asset {
            file_rel: "vendor/htmx.min.js",
            const_name: "HTMX_JS",
            precompress: true,
        },
        Asset {
            file_rel: "vendor/htmx-ext-sse.min.js",
            const_name: "HTMX_EXT_SSE_JS",
            precompress: true,
        },
        Asset {
            file_rel: "vendor/htmx-ext-response-targets.min.js",
            const_name: "HTMX_EXT_RESPONSE_TARGETS_JS",
            precompress: true,
        },
        Asset {
            file_rel: "vendor/htmx-ext-preload.min.js",
            const_name: "HTMX_EXT_PRELOAD_JS",
            precompress: true,
        },
        Asset {
            file_rel: "vendor/alpine.min.js",
            const_name: "ALPINE_JS",
            precompress: true,
        },
        // IBM Plex woff2 — each per-weight + per-subset variant gets
        // its own constant. The names track the file naming convention
        // so a reader can map binary → URL → manifest at a glance.
        // `precompress: false` — woff2 is brotli-compressed at the
        // container level; further brotli/gzip would inflate.
        Asset {
            file_rel: "fonts/ibm-plex-sans-latin-400.woff2",
            const_name: "FONT_PLEX_SANS_LATIN_400",
            precompress: false,
        },
        Asset {
            file_rel: "fonts/ibm-plex-sans-latin-ext-400.woff2",
            const_name: "FONT_PLEX_SANS_LATIN_EXT_400",
            precompress: false,
        },
        Asset {
            file_rel: "fonts/ibm-plex-sans-latin-700.woff2",
            const_name: "FONT_PLEX_SANS_LATIN_700",
            precompress: false,
        },
        Asset {
            file_rel: "fonts/ibm-plex-sans-latin-ext-700.woff2",
            const_name: "FONT_PLEX_SANS_LATIN_EXT_700",
            precompress: false,
        },
        Asset {
            file_rel: "fonts/ibm-plex-mono-latin-400.woff2",
            const_name: "FONT_PLEX_MONO_LATIN_400",
            precompress: false,
        },
        Asset {
            file_rel: "fonts/ibm-plex-mono-latin-ext-400.woff2",
            const_name: "FONT_PLEX_MONO_LATIN_EXT_400",
            precompress: false,
        },
    ];

    let mut manifest = String::from(
        "// Generated by build.rs — do not edit. Maps each served asset \
         to its content-hashed `/static/...` URL per ADR-020 §I.\n\n",
    );
    let mut hashed_urls: HashMap<&str, String> = HashMap::new();

    for asset in BASE_ASSETS {
        let hashed_rel = hash_rename_and_compress(&static_dir, asset.file_rel, asset.precompress);
        writeln!(
            manifest,
            "pub const {}: &str = \"/static/{}\";",
            asset.const_name, hashed_rel,
        )
        .unwrap();
        hashed_urls.insert(asset.const_name, format!("/static/{hashed_rel}"));
    }

    // Generate `fonts.css` from the hashed font URLs. The @font-face
    // declarations reference the hashed paths so the runtime fetches
    // exactly the bytes that landed under their immutable cache entry.
    // `unicode-range` per subset lets the browser skip the Latin Ext
    // file when a page renders only ASCII — important for the marketing
    // surface; aviation safety surfaces with German / Czech / Polish
    // names actually need the extended range. `font-display: swap`
    // surfaces fallback text immediately so the no-JS aviation safety
    // floor (ADR-020 §F) renders before the woff2 download completes.
    let fonts_css = render_fonts_css(&hashed_urls);
    let fonts_css_path = static_dir.join("fonts.css");
    std::fs::write(&fonts_css_path, fonts_css).unwrap_or_else(|e| {
        eprintln!("build.rs: write fonts.css failed: {e}");
        exit(1);
    });

    let fonts_css_hashed = hash_rename_and_compress(&static_dir, "fonts.css", true);
    writeln!(
        manifest,
        "pub const FONTS_CSS: &str = \"/static/{fonts_css_hashed}\";",
    )
    .unwrap();

    std::fs::write(out_dir.join("asset_manifest.rs"), manifest)
        .expect("write asset_manifest.rs to OUT_DIR");
}

/// Copy a single vendored file from `node_modules/` into `static/`.
/// Pulled out so both the JS bundle loop and the woff2 font loop share
/// the same error-formatting path.
fn copy_vendor_file(manifest_dir: &Path, src_rel: &str, dst_rel: &str) {
    let src_path = manifest_dir.join(src_rel);
    let dst_path = manifest_dir.join(dst_rel);
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

/// Read `static/{file_rel}`, hash its bytes, rename in place to
/// `static/{stem}-{hash}.{ext}`, and (if `precompress`) emit `.br` + `.gz`
/// siblings next to the hashed file. Returns the hashed relative path.
///
/// The two precompressed siblings are what the runtime negotiates against
/// `Accept-Encoding`: `tower-http`'s `ServeDir` finds them via
/// `.precompressed_br()` / `.precompressed_gzip()`; the embedded handler
/// walks the same naming scheme manually. `.br` first / `.gz` second
/// mirrors the priority order documented in the route comment.
fn hash_rename_and_compress(static_dir: &Path, file_rel: &str, precompress: bool) -> String {
    let src_path = static_dir.join(file_rel);
    let bytes = std::fs::read(&src_path).unwrap_or_else(|e| {
        eprintln!("build.rs: read {} failed: {}", src_path.display(), e);
        exit(1);
    });
    let hash = short_hash(&bytes);
    let hashed_rel = inject_hash(file_rel, &hash);
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
    if precompress {
        write_zstd(&append_extension(&dst_path, "zst"), &bytes);
        write_brotli(&append_extension(&dst_path, "br"), &bytes);
        write_gzip(&append_extension(&dst_path, "gz"), &bytes);
    }
    hashed_rel
}

/// Compress `bytes` with zstd at level 22 (maximum). Same once-at-build
/// trade-off as brotli q11 — slowest at compile, smallest on the wire.
/// zstd sits at the top of the `Accept-Encoding` negotiation priority
/// (Chrome 123+ / Firefox 126+ advertise it).
fn write_zstd(path: &Path, bytes: &[u8]) {
    let compressed = zstd::encode_all(bytes, 22).unwrap_or_else(|e| {
        eprintln!("build.rs: zstd encode {} failed: {e}", path.display());
        exit(1);
    });
    std::fs::write(path, &compressed).unwrap_or_else(|e| {
        eprintln!("build.rs: write {} failed: {e}", path.display());
        exit(1);
    });
}

/// Compress `bytes` with brotli at quality 11 (maximum) — slowest at build
/// time, smallest on the wire. We pay the CPU once per build and reuse the
/// `.br` file across every cached response, so the build-time cost is
/// negligible compared with the lifetime bandwidth savings.
fn write_brotli(path: &Path, bytes: &[u8]) {
    let mut compressed = Vec::with_capacity(bytes.len());
    // CompressorWriter::new(writer, buffer_size, quality 0-11, lgwin 10-24).
    // 4096 buffer + quality 11 + lgwin 22 (= 4MB window) is the standard
    // "max compression for static asset" knob set.
    let mut encoder = brotli::CompressorWriter::new(&mut compressed, 4096, 11, 22);
    encoder.write_all(bytes).unwrap_or_else(|e| {
        eprintln!("build.rs: brotli encode {} failed: {e}", path.display());
        exit(1);
    });
    drop(encoder);
    std::fs::write(path, &compressed).unwrap_or_else(|e| {
        eprintln!("build.rs: write {} failed: {e}", path.display());
        exit(1);
    });
}

/// Compress `bytes` with gzip at level 9 (`Compression::best`). Same
/// once-at-build trade-off as brotli; gzip is the universal fallback for
/// clients that don't advertise br in `Accept-Encoding`.
fn write_gzip(path: &Path, bytes: &[u8]) {
    let mut compressed = Vec::with_capacity(bytes.len());
    let mut encoder = flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::best());
    encoder.write_all(bytes).unwrap_or_else(|e| {
        eprintln!("build.rs: gzip encode {} failed: {e}", path.display());
        exit(1);
    });
    encoder.finish().unwrap_or_else(|e| {
        eprintln!("build.rs: gzip finish {} failed: {e}", path.display());
        exit(1);
    });
    std::fs::write(path, &compressed).unwrap_or_else(|e| {
        eprintln!("build.rs: write {} failed: {e}", path.display());
        exit(1);
    });
}

/// Append `ext` (without a leading dot) to the path's full file name —
/// e.g. `app-abc12345.css` → `app-abc12345.css.br`. `Path::with_extension`
/// would REPLACE `.css` instead, which is the wrong behaviour for
/// double-extension precompressed sidecars.
fn append_extension(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

/// `unicode-range` for the Latin subset — covers Western European
/// languages without diacriticals beyond the base Latin-1 supplement.
/// Source: Google Fonts CSS API "latin" range, widely-adopted as the
/// de-facto split between "ASCII web" and "diacritical Europe".
const UNICODE_RANGE_LATIN: &str = "U+0000-00FF, U+0131, U+0152-0153, \
    U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, \
    U+2000-206F, U+2074, U+20AC, U+2122, U+2191, U+2193, U+2212, \
    U+2215, U+FEFF, U+FFFD";

/// `unicode-range` for the Latin Extended subset — Central + Eastern
/// European diacriticals (German, Czech, Polish, etc.) plus Vietnamese
/// / IPA combining marks. Aviation safety surfaces with operator names
/// from these locales need this; ASCII-only surfaces skip the download.
const UNICODE_RANGE_LATIN_EXT: &str = "U+0100-02AF, U+0300-0301, \
    U+0303-0304, U+0308-0309, U+0323, U+0329, U+1E00-1EFF, U+2020, \
    U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF";

/// Each woff2 variant we serve as a separate browser-addressable
/// @font-face block. Keyed by the manifest const name so render
/// resolves the hashed URL at emission time.
struct FontFace {
    family: &'static str,
    weight: u32,
    subset_range: &'static str,
    url_const: &'static str,
}

const FONT_FACES: &[FontFace] = &[
    FontFace {
        family: "IBM Plex Sans",
        weight: 400,
        subset_range: UNICODE_RANGE_LATIN,
        url_const: "FONT_PLEX_SANS_LATIN_400",
    },
    FontFace {
        family: "IBM Plex Sans",
        weight: 400,
        subset_range: UNICODE_RANGE_LATIN_EXT,
        url_const: "FONT_PLEX_SANS_LATIN_EXT_400",
    },
    FontFace {
        family: "IBM Plex Sans",
        weight: 700,
        subset_range: UNICODE_RANGE_LATIN,
        url_const: "FONT_PLEX_SANS_LATIN_700",
    },
    FontFace {
        family: "IBM Plex Sans",
        weight: 700,
        subset_range: UNICODE_RANGE_LATIN_EXT,
        url_const: "FONT_PLEX_SANS_LATIN_EXT_700",
    },
    FontFace {
        family: "IBM Plex Mono",
        weight: 400,
        subset_range: UNICODE_RANGE_LATIN,
        url_const: "FONT_PLEX_MONO_LATIN_400",
    },
    FontFace {
        family: "IBM Plex Mono",
        weight: 400,
        subset_range: UNICODE_RANGE_LATIN_EXT,
        url_const: "FONT_PLEX_MONO_LATIN_EXT_400",
    },
];

fn render_fonts_css(hashed_urls: &HashMap<&str, String>) -> String {
    let mut out = String::from(
        "/* Generated by build.rs — do not edit. \
         IBM Plex Sans + Mono woff2 with hashed URLs per ADR-020 §I. */\n\n",
    );
    for face in FONT_FACES {
        let url = hashed_urls
            .get(face.url_const)
            .unwrap_or_else(|| panic!("build.rs: missing hashed URL for {}", face.url_const));
        writeln!(
            out,
            "@font-face {{\n  \
                font-family: '{family}';\n  \
                font-style: normal;\n  \
                font-weight: {weight};\n  \
                font-display: swap;\n  \
                src: url('{url}') format('woff2');\n  \
                unicode-range: {range};\n\
             }}",
            family = face.family,
            weight = face.weight,
            url = url,
            range = face.subset_range,
        )
        .unwrap();
    }
    out
}

/// One entry in the served-asset manifest. `file_rel` is relative to
/// `apps/api/static/`; `const_name` becomes the public Rust identifier
/// in the generated `assets` module. `precompress` controls whether
/// `.br` + `.gz` siblings are emitted next to the hashed file — true
/// for text (CSS, JS), false for woff2 (already brotli-compressed at
/// the container level; double-compression inflates).
struct Asset {
    file_rel: &'static str,
    const_name: &'static str,
    precompress: bool,
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
