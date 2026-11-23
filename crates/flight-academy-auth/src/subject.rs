//! Subject — the entity for whom a policy is evaluated. Shape per
//! ADR-001 §C and the `actor_class` refinement in ADR-010 §B.
//!
//! Subject attributes (ratings, medical class) and staff elevation
//! (ADR-010 §C) are added with the features that read them.

use std::collections::BTreeSet;

use flight_academy_core::{TenantId, UserId};

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

/// A role name this build does not know.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownRole;

impl core::fmt::Display for UnknownRole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("unknown role")
    }
}

impl std::error::Error for UnknownRole {}

impl core::str::FromStr for Role {
    type Err = UnknownRole;

    /// Parse a role from its configuration form. Callers that read role
    /// lists (dev auth today, session decoding later) drop unknown names,
    /// so a list written for a newer build grants nothing extra.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tenant-admin" | "tenant_admin" => Ok(Self::TenantAdmin),
            _ => Err(UnknownRole),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Subject {
    pub user_id: UserId,
    pub actor_class: ActorClass,
    /// `None` for `Staff` until elevated (ADR-010 §B); always `Some` for
    /// `Member` calling the product API.
    pub tenant_id: Option<TenantId>,
    pub roles: BTreeSet<Role>,
}
