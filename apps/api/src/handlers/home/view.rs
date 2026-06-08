//! Maud templates for the home / landing surface.
//!
//! Pure render functions — no `axum`, no HTTP types. Each function takes
//! its props as arguments and returns [`maud::Markup`]. Unit-testable
//! without HTTP machinery; reusable across full-page and HTMX-fragment
//! handlers when those land.
//!
//! Conventions:
//!
//! - The function name describes the view (`landing`, not `render_landing`).
//! - The view does not know its URL — the controller (mod.rs) decides
//!   what gets rendered and at which route.
//! - Tailwind class strings live in the template; the controller never
//!   sees CSS class names.

use maud::{DOCTYPE, Markup, html};

/// Landing page — Tailwind-styled HTML shell with the title block and
/// a brief project description. Linked to the compiled Tailwind bundle
/// at `/static/app.css`; the HTML chrome is intentionally minimal until
/// the shared chrome / header / footer helpers land alongside a second
/// HTML surface (ADR-020 §D `handlers/shared/`).
pub fn landing() -> Markup {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke check that the view renders without panicking and includes
    /// the expected surface markers. Demonstrates the view-layer is
    /// HTTP-agnostic — no router, no `app_for_test()`, no `tower::oneshot`.
    /// The integration test in `apps/api/tests/home.rs` covers the full
    /// HTTP round-trip (status, headers, CSP).
    #[test]
    fn landing_renders_full_html_document() {
        let body = landing().into_string();

        assert!(
            body.starts_with("<!DOCTYPE"),
            "must be a full HTML document"
        );
        assert!(body.contains("<title>Flight Academy</title>"));
        assert!(
            body.contains(r#"<link rel="stylesheet" href="/static/app.css">"#),
            "Tailwind-compiled stylesheet must be linked per ADR-020 §E",
        );
    }
}
