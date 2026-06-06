-- Tenants table — the first operational entity per ADR-001 §A. Public
-- addressing via slug per ADR-006 §C; replication metadata per ADR-007
-- §B/§C; tenant types from the domain model §1.2.
--
-- RLS is intentionally OFF on this table. The slug → id resolution must
-- succeed BEFORE the request can enter a tenant-scoped transaction (that
-- transaction's `app.current_tenant` GUC is set from the resolved id).
-- Putting RLS on tenants would create a chicken-and-egg: the lookup needs
-- to read a row to find out which tenant context to set. Authorization
-- for "can this subject see this tenant?" lives at the ABAC policy layer
-- (`TenantOwnership`) per ADR-001 §C, not at the row level.

CREATE TABLE tenants (
    id              uuid        NOT NULL DEFAULT gen_random_uuid(),
    -- DNS-label-shaped: leading lowercase letter, then lowercase
    -- alphanumeric or hyphen, 2-63 chars total. Bounded so a future
    -- subdomain-per-tenant layout (ADR-013 §F mentions per-plane keys; a
    -- per-tenant subdomain is the natural extension) fits inside the
    -- 63-char DNS label limit. Hand-rolled regex rather than pulling the
    -- `validator` crate — every other constraint here is a CHECK too.
    slug            text        NOT NULL CHECK (slug ~ '^[a-z][a-z0-9-]{1,62}$'),
    name            text        NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    -- Aviation operator class per domain model §1.2. Stored as text +
    -- CHECK rather than PG ENUM — matches the audit_events.actor_class
    -- style and makes adding a fourth/fifth operator class a forward-only
    -- ALTER TABLE ... DROP CONSTRAINT / ADD CONSTRAINT instead of an
    -- ALTER TYPE that needs all readers restarted.
    tenant_type     text        NOT NULL CHECK (tenant_type IN ('ato', 'part_145', 'airfield_operator')),
    -- White-label settings (brand colours, logo file id, custom domain
    -- bits). Opaque jsonb in this slice; typed shape lands with the first
    -- consumer.
    settings        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now(),
    -- Bumped by the BEFORE UPDATE trigger below. The updated_since sync
    -- feed (ADR-007 §B1) reads this column to discover what changed.
    updated_at      timestamptz NOT NULL DEFAULT now(),
    -- Soft-delete fields per ADR-007 §C / ADR-016 §B. No hard delete; the
    -- GDPR right-to-erasure answer is crypto-shredding the per-tenant DEK
    -- (ADR-001 §D), not removing the metadata row. deletion_reason stays
    -- free-form for now; the enum lands when the delete endpoint does
    -- (sub-slice B).
    deleted_at      timestamptz,
    deletion_reason text,
    PRIMARY KEY (id),
    -- Either both deletion fields are set or neither is. Rules out the
    -- mid-state "deleted but no reason recorded".
    CONSTRAINT tenants_deletion_consistency CHECK (
        (deleted_at IS NULL AND deletion_reason IS NULL)
        OR
        (deleted_at IS NOT NULL AND deletion_reason IS NOT NULL)
    )
);

-- Partial unique index per ADR-007 §E: soft-deleted tenants release their
-- slug for reuse. The phishing hazard of reusing a published slug is
-- documented in the domain model; mitigation (slug-reuse grace period via
-- retention rules) is deferred until the retention-rules resource lands.
CREATE UNIQUE INDEX tenants_slug_unique
    ON tenants (slug)
    WHERE deleted_at IS NULL;

-- Replication watermark index per ADR-007 §B1. tenants are not a
-- tenant-owned resource (they ARE the tenant), so the shape is
-- (updated_at, id) rather than the prescribed (tenant_id, updated_at, id)
-- for tenant-owned rows.
CREATE INDEX tenants_updated_at_id_idx
    ON tenants (updated_at, id);

-- updated_at maintenance. Regular table (not partitioned), so FOR EACH
-- ROW is fine here — the partitioned-table restriction that forced
-- statement triggers on audit_events does not apply.
CREATE FUNCTION tenants_bump_updated_at() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

CREATE TRIGGER tenants_bump_updated_at
    BEFORE UPDATE ON tenants
    FOR EACH ROW EXECUTE FUNCTION tenants_bump_updated_at();

-- Grants. Read-only for this slice; INSERT/UPDATE land with the
-- Staff-actor CRUD endpoints (sub-slice B). app_migrator already owns
-- the table via the ROLE the migration runs as.
GRANT SELECT ON tenants TO app_api;
