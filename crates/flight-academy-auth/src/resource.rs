//! Resource — the target of a policy evaluation. Shape per ADR-001 §C.

use uuid::Uuid;

/// Stub variant set — grows as domain resources land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    AuditEvent,
    Tenant,
}

/// Stub. Per-resource attributes (state, ownership lineage, sensitivity
/// class per ADR-008 §B) land with the resources that need them.
#[derive(Clone, Debug, Default)]
pub struct ResourceAttributes;

#[derive(Clone, Debug)]
pub struct Resource {
    pub tenant_id: Uuid,
    pub kind: ResourceKind,
    pub owner: Option<Uuid>,
    pub attributes: ResourceAttributes,
}
