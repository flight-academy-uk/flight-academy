//! `/` — landing page; the first MASH HTML surface (ADR-020).
//!
//! Server-rendered Maud markup linking the Tailwind-compiled stylesheet
//! served at `/static/app.css`. This is the smallest end-to-end demonstration
//! that the MASH stack (Maud + Axum + Tailwind) renders. HTMX + Alpine
//! vendoring, content-hashed asset URLs, the per-resource `view.rs` template
//! module pattern (ADR-020 §D), and the no-JS verification gate (ADR-020 §N)
//! follow in subsequent slices.
//!
//! Not in the OpenAPI contract per ADR-020 §A — the HTML surface is parallel
//! to `/api/v1/*`, not a member of it. Registered as a plain Axum route in
//! `crate::build` after `OpenApiRouter::split_for_parts`.

use maud::{DOCTYPE, Markup, html};

/// Render the landing page.
pub async fn get() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Flight Academy" }
                link rel="stylesheet" href="/static/app.css";
            }
            body class="min-h-screen flex items-center justify-center bg-white text-slate-900" {
                main class="max-w-prose px-6 py-12" {
                    h1 class="text-4xl font-semibold tracking-tight" { "Flight Academy" }
                    p class="mt-4 text-lg text-slate-600" {
                        "Open-source aviation operations platform — flight schools, maintenance organisations, airfields."
                    }
                    p class="mt-2 text-sm text-slate-500" {
                        "Server-rendered HTML — Maud + Axum + Tailwind today; HTMX + Alpine wiring follows per ADR-020. Pre-alpha."
                    }
                }
            }
        }
    }
}
