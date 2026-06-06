//! Subject — the entity for whom a policy is evaluated. Shape per
//! ADR-001 §C and the `actor_class` refinement in ADR-010 §B.
//!
//! WS#4 populates `user_id`, `actor_class`, `tenant_id`; the richer slots
//! (`roles`, `attributes`, `elevation`) are stub types until the real auth
//! and aviation-attribute machinery lands.

use std::collections::BTreeSet;

use uuid::Uuid;

/// Three actor classes per ADR-010 §B. `Member` is the default — pilots,
/// students, instructors, ATO staff acting on tenant resources. `Staff` is
/// the platform operator plane (out of band for the product API per
/// ADR-010 §B; staff can only call the admin-plane handlers). `System` is
/// background jobs that the audit chain attributes when no human is the
/// actor (cron, scheduled migrations, partition manager).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorClass {
    Member,
    Staff,
    System,
}

/// Membership / staff role taxonomy. Grows as endpoints land. Today's
/// set:
///
/// * `TenantAdmin` — a member with management rights over their own
///   tenant (rename, update settings, soft-delete). Granted by another
///   tenant-admin (bootstrap by Staff per the staff-plane work to come).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Role {
    TenantAdmin,
}

impl Role {
    /// Parse a role from its config-file string form. Returns `None` for
    /// unknown values; callers (dev_auth, future session decoders)
    /// silently drop unknowns so a config file ahead of the binary's
    /// known set degrades safely rather than failing open.
    pub fn from_str_known(s: &str) -> Option<Self> {
        match s {
            "tenant-admin" | "tenant_admin" => Some(Self::TenantAdmin),
            _ => None,
        }
    }
}

/// Stub. Real subject attributes (medical class, ratings, instructor
/// seniority — the things that make aviation ABAC interesting per
/// ADR-001 §C) land with the aviation domain crate.
#[derive(Clone, Debug, Default)]
pub struct SubjectAttributes;

/// Stub. Real elevation grants per ADR-010 §C land with the staff plane.
#[derive(Clone, Debug)]
pub struct Elevation;

#[derive(Clone, Debug)]
pub struct Subject {
    pub user_id: Uuid,
    pub actor_class: ActorClass,
    /// `None` for `Staff` until elevated (ADR-010 §B); always `Some` for
    /// `Member` calling the product API.
    pub tenant_id: Option<Uuid>,
    pub roles: BTreeSet<Role>,
    pub attributes: SubjectAttributes,
    pub elevation: Option<Elevation>,
}
