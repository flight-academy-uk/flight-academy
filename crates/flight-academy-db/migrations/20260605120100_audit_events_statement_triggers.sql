-- Refactor audit_events immutability triggers from ROW to STATEMENT level.
--
-- Context: PG 17 tightened CREATE TRIGGER on partitioned tables — `FOR EACH
-- ROW` is no longer allowed (only `FOR EACH STATEMENT`). PG 16 permits both.
-- The init migration (20260605000000_init.sql) used ROW triggers, which
-- worked on PG 16 but rejects on PG 17+. This migration replaces the two
-- ROW triggers with STATEMENT triggers; functionally equivalent for our use
-- case (we block the whole statement, not inspect rows). The TRUNCATE
-- trigger was already STATEMENT-level — TRUNCATE has no per-row semantics
-- — so it stays as-is.
--
-- This is forward-only-correct per ADR-003 §A: the prior migration stays
-- in place; this one corrects the trigger granularity. Production CNPG can
-- now run on PG 16, 17, or 18 — set the cluster's `imageName` to whichever
-- major version is current at deploy time.

DROP TRIGGER audit_events_no_update ON audit_events;
DROP TRIGGER audit_events_no_delete ON audit_events;

CREATE TRIGGER audit_events_no_update
    BEFORE UPDATE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_events_immutable();

CREATE TRIGGER audit_events_no_delete
    BEFORE DELETE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_events_immutable();
