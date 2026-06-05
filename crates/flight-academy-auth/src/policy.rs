//! Policy evaluation. `Decision` per ADR-001 §C; `Policy` trait the same.
//!
//! WS#4 ships one concrete policy — [`TenantOwnership`] — which checks
//! that the calling subject's tenant matches the resource's tenant. It is
//! the minimum policy any tenant-scoped product-API call must pass; richer
//! policies (role-gated, attribute-gated) layer on top in later commits.

use crate::{Action, Resource, Subject};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Permit,
    Deny {
        reason: String,
    },
    /// The policy is silent on this (subject, action, resource) triple —
    /// composed policies treat this as "ask the next layer". A request
    /// where every applicable policy returns `NotApplicable` is denied
    /// by default (deny-by-default — ADR-001 §C).
    NotApplicable,
}

pub trait Policy {
    fn permit(&self, subject: &Subject, action: Action, resource: &Resource) -> Decision;
}

/// Baseline policy: caller's `tenant_id` must match the resource's
/// `tenant_id`. Every tenant-scoped product-API call must pass this before
/// any action-specific policy is even consulted (ADR-006 §C: the
/// `{tenant}` path segment must match the caller's tenant or the request
/// is `403`).
pub struct TenantOwnership;

impl Policy for TenantOwnership {
    fn permit(&self, subject: &Subject, _action: Action, resource: &Resource) -> Decision {
        match subject.tenant_id {
            Some(t) if t == resource.tenant_id => Decision::Permit,
            Some(_) => Decision::Deny {
                reason: "subject tenant does not match resource tenant".to_string(),
            },
            None => Decision::Deny {
                reason: "subject has no tenant context".to_string(),
            },
        }
    }
}
