//! flight-academy-auth — ABAC primitives per ADR-001 §C and ADR-010 §B.
//!
//! Walking-skeleton scope (WS#4): types + one concrete policy. Passwordless
//! sessions (ADR-001 §F), WebAuthn / magic-link / push (ADR-013), and the
//! richer role/attribute taxonomies land alongside the subsystems that
//! need them.

mod policy;
mod resource;
mod subject;

pub use policy::{Decision, Policy, TenantAdministration, TenantOwnership, authorise};
pub use resource::{Resource, ResourceKind};
pub use subject::{ActorClass, Role, Subject, UnknownRole};

/// Grows as endpoints land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    ListAuditEvents,
    ReadTenant,
    UpdateTenant,
    DeleteTenant,
}
