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

/// Landing page — Tailwind-styled HTML shell with the title block, a
/// brief project description, and a small MASH-stack proof-of-life
/// section that demonstrates HTMX fetching a server-rendered fragment.
///
/// Script tags load the vendored HTMX bundle + its three extensions
/// (sse, response-targets, preload) and Alpine's CSP-safe build. The
/// extensions are activated via `hx-ext="..."` on the body; Alpine is
/// loaded so future components can register via `Alpine.data(...)` from
/// external scripts without any CSP relaxation. No inline `x-data`
/// expressions on this page yet — when they land they will resolve to
/// registered component names per the CSP-safe Alpine API
/// (see <https://alpinejs.dev/advanced/csp>).
pub fn landing() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Flight Academy" }
                link rel="stylesheet" href="/static/app.css";
                // Vendored bundles per ADR-020 §F. `defer` so the HTML
                // parses before script execution; HTMX activates on
                // DOMContentLoaded regardless.
                script src="/static/vendor/htmx.min.js" defer {}
                script src="/static/vendor/htmx-ext-sse.min.js" defer {}
                script src="/static/vendor/htmx-ext-response-targets.min.js" defer {}
                script src="/static/vendor/htmx-ext-preload.min.js" defer {}
                script src="/static/vendor/alpine.min.js" defer {}
            }
            body
                class="min-h-screen flex items-center justify-center bg-white text-slate-900"
                hx-ext="sse,response-targets,preload" {
                main class="max-w-prose px-6 py-12" {
                    h1 class="text-4xl font-semibold tracking-tight" { "Flight Academy" }
                    p class="mt-4 text-lg text-slate-600" {
                        "Open-source aviation operations platform — flight schools, maintenance organisations, airfields."
                    }
                    p class="mt-2 text-sm text-slate-500" {
                        "Server-rendered HTML — Maud + Axum + Tailwind + HTMX 2.x (Alpine 3.x vendored). Pre-alpha."
                    }

                    section class="mt-10 rounded-lg border border-slate-200 p-6" {
                        h2 class="text-base font-semibold" { "MASH stack proof of life" }
                        p class="mt-1 text-sm text-slate-500" {
                            "Clicking the button below issues an HTMX GET against the server, which renders a Maud fragment containing a fresh UUID v7. The fragment is swapped into the slot below."
                        }
                        div class="mt-4 flex flex-col gap-3" {
                            button
                                class="self-start rounded bg-slate-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-slate-700 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-900"
                                hx-get="/_hx/home/server-id"
                                hx-target="#server-id-slot"
                                hx-swap="innerHTML" {
                                "Ask the server for a fresh ID"
                            }
                            div id="server-id-slot" class="text-sm font-mono text-slate-500" {
                                "(click the button)"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// HTMX fragment swapped into `#server-id-slot` when the landing-page
/// button is clicked. Holds the server-generated id and nothing else;
/// no `<html>`, no `<head>`, no scripts. Returned as a UTF-8 HTML
/// document with the same `text/html` MIME so HTMX swaps it directly
/// per ADR-020 §G.
pub fn server_id_fragment(id: &str) -> Markup {
    html! {
        span class="text-slate-900" { (id) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke check that the landing view renders without panicking and
    /// includes the expected surface markers. Demonstrates the view
    /// layer is HTTP-agnostic — no router, no `app_for_test()`, no
    /// `tower::oneshot`. The integration test in `apps/api/tests/home.rs`
    /// covers the full HTTP round-trip (status, headers, CSP).
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

    #[test]
    fn landing_includes_vendored_mash_scripts() {
        let body = landing().into_string();

        // Vendored bundles must be referenced — without them HTMX
        // would not initialise and the demo button would 404 on its
        // hx-get because nothing parses the attribute.
        for expected in [
            r#"<script src="/static/vendor/htmx.min.js" defer></script>"#,
            r#"<script src="/static/vendor/htmx-ext-sse.min.js" defer></script>"#,
            r#"<script src="/static/vendor/htmx-ext-response-targets.min.js" defer></script>"#,
            r#"<script src="/static/vendor/htmx-ext-preload.min.js" defer></script>"#,
            r#"<script src="/static/vendor/alpine.min.js" defer></script>"#,
        ] {
            assert!(
                body.contains(expected),
                "landing must reference vendored bundle:\n  expected: {expected}\n  body: {body}"
            );
        }

        // HTMX extensions are activated via `hx-ext` on the body.
        assert!(
            body.contains(r#"hx-ext="sse,response-targets,preload""#),
            "body must activate the three HTMX extensions",
        );

        // The proof-of-life button uses HTMX attributes. Confirming
        // the attribute is present here keeps the integration test
        // focused on the HTTP round-trip.
        assert!(
            body.contains(r#"hx-get="/_hx/home/server-id""#),
            "demo button must wire to the fragment endpoint",
        );
    }

    #[test]
    fn server_id_fragment_is_a_bare_span() {
        let body = server_id_fragment("01928f3e-1234-7abc-9def-000000000001").into_string();

        // Fragment must NOT be a full document — HTMX swaps it as
        // inner HTML; a leading <!DOCTYPE would break the swap.
        assert!(!body.contains("<!DOCTYPE"));
        assert!(!body.contains("<html"));
        assert!(!body.contains("<head"));
        assert!(!body.contains("<body"));

        // The id itself appears in the fragment.
        assert!(body.contains("01928f3e-1234-7abc-9def-000000000001"));
    }
}
