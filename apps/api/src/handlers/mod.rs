//! Per-resource HTTP handlers. One module per resource — handler
//! functions, their wire-shape response types, and any resource-local
//! helpers (e.g. `resolve_tenant`) live next to each other rather than in
//! one growing `lib.rs`. Router assembly + the OpenAPI document
//! continue to live in `lib.rs`.
//!
//! # Layout pattern
//!
//! - **JSON-only resources** stay flat: `resource.rs` with handler
//!   functions annotated `#[utoipa::path(...)]` so they land in the
//!   OpenAPI document per ADR-018.
//! - **Resources with HTML rendering** become a subdirectory per
//!   ADR-020 §D — `resource/mod.rs` for the handler (controller; owns
//!   HTTP concerns, CSP, response shaping) and `resource/view.rs` for
//!   the Maud template (pure render, no `axum` or HTTP types). The
//!   split makes the view layer unit-testable without spinning up the
//!   router and keeps Tailwind class strings out of the controller.
//! - **HTML and JSON handlers for the same resource** sit side-by-side
//!   in `resource/mod.rs`. Disambiguate at the function name (Rails-ish
//!   convention): `tenant_get` / `tenant_patch` for JSON,
//!   `tenant_show` / `tenant_index` for HTML. The `#[utoipa::path]`
//!   attribute is the unmistakable visual marker for the OpenAPI
//!   contract surface.
//! - **Cross-resource layout helpers** (shared `<head>`, header, footer,
//!   icons) go under `shared/` once a second HTML surface needs them
//!   — premature today with a single landing page.

pub mod audit_events;
pub mod health;
pub mod home;
pub mod tenants;
