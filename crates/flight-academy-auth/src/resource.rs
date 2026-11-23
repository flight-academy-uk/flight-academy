//! Resource — the target of a policy evaluation. Shape per ADR-001 §C.

use flight_academy_core::TenantId;

/// Grows as domain resources land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    AuditEvent,
    Tenant,
}

/// A resource the caller can already see. Policies decide what the caller
/// may do with it; whether it is visible at all is settled earlier, by the
/// query that found it (ADR-033).
#[derive(Clone, Debug)]
pub struct Resource {
    pub tenant_id: TenantId,
    pub kind: ResourceKind,
}
