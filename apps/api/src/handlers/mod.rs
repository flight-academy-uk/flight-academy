//! Per-resource HTTP handlers. One module per resource — handler
//! functions, their wire-shape response types, and any resource-local
//! helpers (e.g. `resolve_tenant`) live next to each other rather than in
//! one growing `lib.rs`. Router assembly + the OpenAPI document
//! continue to live in `lib.rs`.

pub mod audit_events;
pub mod health;
pub mod tenants;
