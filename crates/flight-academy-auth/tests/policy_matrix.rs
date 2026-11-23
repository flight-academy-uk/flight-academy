//! Pure-logic permit/deny matrix for the WS#4 baseline policy. No I/O,
//! no DB — the ABAC layer is data-in / decision-out.

use std::collections::BTreeSet;

use flight_academy_auth::{
    Action, ActorClass, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Role,
    Subject, SubjectAttributes, TenantAdministration, TenantOwnership,
};
use flight_academy_core::{TenantId, UserId};

fn subject(actor_class: ActorClass, tenant_id: Option<TenantId>) -> Subject {
    Subject {
        user_id: UserId::random(),
        actor_class,
        tenant_id,
        roles: BTreeSet::new(),
        attributes: SubjectAttributes,
        elevation: None,
    }
}

fn admin_subject(tenant_id: TenantId) -> Subject {
    let mut s = subject(ActorClass::Member, Some(tenant_id));
    s.roles.insert(Role::TenantAdmin);
    s
}

fn resource(tenant_id: TenantId) -> Resource {
    Resource {
        tenant_id,
        kind: ResourceKind::AuditEvent,
        owner: None,
        attributes: ResourceAttributes,
    }
}

#[test]
fn tenant_ownership_permits_matching_tenant() {
    let t = TenantId::random();
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
        &subject(ActorClass::Member, Some(TenantId::random())),
        Action::ListAuditEvents,
        &resource(TenantId::random()),
    );
    assert!(matches!(decision, Decision::Deny { .. }));
}

#[test]
fn tenant_ownership_denies_subject_without_tenant() {
    let decision = TenantOwnership.permit(
        &subject(ActorClass::Staff, None),
        Action::ListAuditEvents,
        &resource(TenantId::random()),
    );
    assert!(matches!(decision, Decision::Deny { .. }));
}

#[test]
fn tenant_administration_permits_admin_of_matching_tenant() {
    let t = TenantId::random();
    let decision =
        TenantAdministration.permit(&admin_subject(t), Action::UpdateTenant, &resource(t));
    assert_eq!(decision, Decision::Permit);
}

#[test]
fn tenant_administration_denies_non_admin_member() {
    // Member of the right tenant, but no tenant-admin role: ownership
    // passes, role check fails.
    let t = TenantId::random();
    let decision = TenantAdministration.permit(
        &subject(ActorClass::Member, Some(t)),
        Action::UpdateTenant,
        &resource(t),
    );
    match decision {
        Decision::Deny { reason } => assert!(reason.contains("tenant-admin")),
        other => panic!("expected Deny with tenant-admin reason, got {other:?}"),
    }
}

#[test]
fn tenant_administration_denies_admin_of_other_tenant() {
    // The role doesn't unlock cross-tenant administration — ownership
    // fires first, denies on tenant mismatch.
    let admin_t = TenantId::random();
    let target_t = TenantId::random();
    let decision = TenantAdministration.permit(
        &admin_subject(admin_t),
        Action::DeleteTenant,
        &resource(target_t),
    );
    match decision {
        Decision::Deny { reason } => assert!(reason.contains("does not match")),
        other => panic!("expected Deny with tenant-mismatch reason, got {other:?}"),
    }
}

#[test]
fn tenant_administration_denies_subject_without_tenant() {
    let decision = TenantAdministration.permit(
        &subject(ActorClass::Member, None),
        Action::UpdateTenant,
        &resource(TenantId::random()),
    );
    assert!(matches!(decision, Decision::Deny { .. }));
}
