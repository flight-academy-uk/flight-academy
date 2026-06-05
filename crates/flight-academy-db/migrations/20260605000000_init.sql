-- Initial schema: roles + audit_events (range-partitioned, hash-chain ready).
--
-- Roles per ADR-002 §F (subset — app_read_only deferred until first SELECT-only
-- consumer lands; see commit message). audit_events per ADR-009 §C/§E:
-- per-tenant + per-user + platform hash chains; monthly range partitioning on
-- occurred_at. RLS policies attach in a later migration when ABAC subjects
-- exist and `app.current_tenant` GUC has a writer.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Roles. NOLOGIN here — login attributes (password / cert / IAM auth) are
-- attached by the operator alongside the role grant, differently in hosted
-- (CNPG + IRSA) and self-host (docker-compose env), but the role identities
-- and privilege boundary are identical.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_migrator') THEN
        CREATE ROLE app_migrator NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_api') THEN
        CREATE ROLE app_api NOLOGIN;
    END IF;
END
$$;

-- audit_events: tamper-evident compliance trail. INSERT-only at the SQL
-- level (trigger below); the hash chain depends on row immutability.
-- Partitioned by month on occurred_at; PK includes the partition key as PG
-- declarative partitioning requires.
CREATE TABLE audit_events (
    id           uuid        NOT NULL DEFAULT gen_random_uuid(),
    occurred_at  timestamptz NOT NULL DEFAULT now(),
    actor_class  text        NOT NULL CHECK (actor_class IN ('member', 'staff', 'system')),
    actor_id     uuid,
    tenant_id    uuid,
    chain_kind   text        NOT NULL CHECK (chain_kind IN ('tenant', 'user', 'platform')),
    chain_id     uuid,
    prev_hash    bytea,
    payload      jsonb       NOT NULL,
    payload_hash bytea       NOT NULL,
    PRIMARY KEY (occurred_at, id)
) PARTITION BY RANGE (occurred_at);

-- Chain-walk index: verification jobs walk `WHERE chain_kind = ? AND
-- chain_id = ? ORDER BY occurred_at` to recompute prev_hash linkage.
-- Propagated to partitions automatically (PG 11+).
CREATE INDEX audit_events_chain_idx
    ON audit_events (chain_kind, chain_id, occurred_at);

-- Immutability enforcement. Hash chains require row immutability; the trigger
-- raises on any UPDATE/DELETE/TRUNCATE, including from app_migrator. If a
-- future migration needs to add a column, that is DDL (ALTER TABLE) which
-- does not fire row triggers — but no migration may UPDATE audit rows.
CREATE FUNCTION audit_events_immutable() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is INSERT-only (ADR-009 §A)';
END;
$$;

CREATE TRIGGER audit_events_no_update
    BEFORE UPDATE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_immutable();

CREATE TRIGGER audit_events_no_delete
    BEFORE DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_immutable();

CREATE TRIGGER audit_events_no_truncate
    BEFORE TRUNCATE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_events_immutable();

-- Initial partitions: current and next month. An automatic partition manager
-- (creating next-month partitions before each month begins per ADR-009 §E)
-- lands when a scheduled-job mechanism exists. Until then, a migration adds
-- the next partition manually before the month rolls.
CREATE TABLE audit_events_2026_06 PARTITION OF audit_events
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');

CREATE TABLE audit_events_2026_07 PARTITION OF audit_events
    FOR VALUES FROM ('2026-07-01') TO ('2026-08-01');

-- Grants. app_api gets row-level access; RLS policies attach in a later
-- migration when chain_id can be scoped to the current session's tenant
-- via `app.current_tenant` GUC (ADR-001 §C / ADR-010 §B).
GRANT INSERT, SELECT ON audit_events TO app_api;
