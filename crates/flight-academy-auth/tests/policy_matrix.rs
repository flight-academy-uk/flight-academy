//! Pure-logic permit/deny matrix for the WS#4 baseline policy. No I/O,
//! no DB — the ABAC layer is data-in / decision-out.

use std::collections::BTreeSet;

use flight_academy_auth::{
    Action, ActorClass, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Subject,
    SubjectAttributes, TenantOwnership,
};
use uuid::Uuid;

fn subject(actor_class: ActorClass, tenant_id: Option<Uuid>) -> Subject {
    Subject {
        user_id: Uuid::new_v4(),
        actor_class,
        tenant_id,
        roles: BTreeSet::new(),
        attributes: SubjectAttributes,
        elevation: None,
    }
}

fn resource(tenant_id: Uuid) -> Resource {
    Resource {
        tenant_id,
        kind: ResourceKind::AuditEvent,
        owner: None,
        attributes: ResourceAttributes,
    }
}

#[test]
fn tenant_ownership_permits_matching_tenant() {
    let t = Uuid::new_v4();
    let decision = TenantOwnership.permit(
        &subject(ActorClass::Member, Some(t)),
        Action::ListAuditEvents,
        &resource(t),
    );
    assert_eq!(decision, Decision::Permit);
}

#[test]
fn tenant_ownership_denies_mismatched_tenant() {
    let decision = TenantOwnership.permit(
        &subject(ActorClass::Member, Some(Uuid::new_v4())),
        Action::ListAuditEvents,
        &resource(Uuid::new_v4()),
    );
    assert!(matches!(decision, Decision::Deny { .. }));
}

#[test]
fn tenant_ownership_denies_subject_without_tenant() {
    let decision = TenantOwnership.permit(
        &subject(ActorClass::Staff, None),
        Action::ListAuditEvents,
        &resource(Uuid::new_v4()),
    );
    assert!(matches!(decision, Decision::Deny { .. }));
}
