//! Pure-logic permit/deny matrix for the tenant policies. No I/O, no DB:
//! the ABAC layer is data in, decision out.

use std::collections::BTreeSet;

use flight_academy_auth::{
    Action, ActorClass, Decision, Denied, Policy, Resource, ResourceKind, Role, Subject,
    TenantAdministration, TenantOwnership, UnknownRole, authorise,
};
use flight_academy_core::{Error, TenantId, UserId};

fn subject(actor_class: ActorClass, tenant_id: Option<TenantId>) -> Subject {
    Subject {
        user_id: UserId::random(),
        actor_class,
        tenant_id,
        roles: BTreeSet::new(),
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

#[test]
fn tenant_ownership_denies_staff_and_system_even_in_the_tenant() {
    // Staff and system actors do not use the product API (ADR-010 §B), so
    // a tenant context does not let them through.
    let t = TenantId::random();
    for actor_class in [ActorClass::Staff, ActorClass::System] {
        let decision = TenantOwnership.permit(
            &subject(actor_class, Some(t)),
            Action::ReadTenant,
            &resource(t),
        );
        match decision {
            Decision::Deny { reason } => assert!(reason.contains("only members")),
            other => panic!("expected Deny for {actor_class:?}, got {other:?}"),
        }
    }
}

#[test]
fn authorise_maps_permit_to_ok_and_deny_to_denied() {
    let t = TenantId::random();
    assert!(
        authorise(
            &TenantOwnership,
            &subject(ActorClass::Member, Some(t)),
            Action::ReadTenant,
            &resource(t),
        )
        .is_ok()
    );
    let denied = authorise(
        &TenantAdministration,
        &subject(ActorClass::Member, Some(t)),
        Action::UpdateTenant,
        &resource(t),
    );
    assert_eq!(
        denied,
        Err(Denied {
            reason: "subject is not a tenant-admin"
        })
    );
    assert!(matches!(Error::from(denied.unwrap_err()), Error::Forbidden));
}

#[test]
fn roles_parse_from_their_configuration_names() {
    assert_eq!("tenant-admin".parse::<Role>(), Ok(Role::TenantAdmin));
    assert_eq!("tenant_admin".parse::<Role>(), Ok(Role::TenantAdmin));
    assert_eq!("superuser".parse::<Role>(), Err(UnknownRole));
    assert_eq!("".parse::<Role>(), Err(UnknownRole));
}
