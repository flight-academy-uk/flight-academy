//! Policy evaluation. `Decision` per ADR-001 §C; `Policy` the same.
//!
//! A policy decides what a caller may do with a resource it can already
//! see. Visibility itself is a predicate in the query that loads the
//! resource, so a resource the caller cannot see is never handed to a
//! policy: it is not found (ADR-033). A `Deny` here therefore means a
//! visible resource and a forbidden action, which the HTTP layer reports
//! as `403`.

use flight_academy_core::Error;

use crate::{Action, ActorClass, Resource, Role, Subject};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Permit,
    Deny { reason: &'static str },
}

pub trait Policy {
    fn permit(&self, subject: &Subject, action: Action, resource: &Resource) -> Decision;
}

/// A policy refused the action. The HTTP layer records it and answers
/// `403`; the reason goes to the audit row and debug log, never to the
/// response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Denied {
    pub reason: &'static str,
}

impl From<Denied> for Error {
    fn from(_: Denied) -> Self {
        Error::Forbidden
    }
}

/// Evaluate `policy`, turning a Deny into [`Denied`] and logging its
/// reason at debug level.
pub fn authorise(
    policy: &impl Policy,
    subject: &Subject,
    action: Action,
    resource: &Resource,
) -> Result<(), Denied> {
    match policy.permit(subject, action, resource) {
        Decision::Permit => Ok(()),
        Decision::Deny { reason } => {
            tracing::debug!(?action, kind = ?resource.kind, reason, "policy denied");
            Err(Denied { reason })
        }
    }
}

/// Baseline for every tenant-scoped product-API call: the caller is a
/// member acting in the resource's tenant. Staff and system actors do not
/// use the product API (ADR-010 §B), so they are denied even when a tenant
/// context is present.
pub struct TenantOwnership;

impl Policy for TenantOwnership {
    fn permit(&self, subject: &Subject, _action: Action, resource: &Resource) -> Decision {
        if subject.actor_class != ActorClass::Member {
            return Decision::Deny {
                reason: "only members use the product API",
            };
        }
        match subject.tenant_id {
            Some(t) if t == resource.tenant_id => Decision::Permit,
            Some(_) => Decision::Deny {
                reason: "subject tenant does not match resource tenant",
            },
            None => Decision::Deny {
                reason: "subject has no tenant context",
            },
        }
    }
}

/// Tenant administration: [`TenantOwnership`] plus `Role::TenantAdmin`.
/// Gates the mutations on the tenant resource (PATCH and DELETE today).
/// The role never unlocks another tenant: ownership is checked first.
pub struct TenantAdministration;

impl Policy for TenantAdministration {
    fn permit(&self, subject: &Subject, action: Action, resource: &Resource) -> Decision {
        match TenantOwnership.permit(subject, action, resource) {
            Decision::Permit if subject.roles.contains(&Role::TenantAdmin) => Decision::Permit,
            Decision::Permit => Decision::Deny {
                reason: "subject is not a tenant-admin",
            },
            deny => deny,
        }
    }
}
