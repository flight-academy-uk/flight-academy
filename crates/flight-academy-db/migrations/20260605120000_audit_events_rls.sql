-- Row-level security on audit_events. Tenant-scoped product-API queries
-- see only their own tenant chain's rows. User-chain (`chain_kind='user'`,
-- /me/* per ADR-006 §C) and platform-chain (`chain_kind='platform'`,
-- staff plane per ADR-010 §I) rows are invisible to app_api in this
-- context; those are served via separate connections in their own planes.
--
-- The active tenant is set by Db::begin_tenant — a transaction-scoped
--   SET LOCAL app.current_tenant = '<uuid>';
-- followed by a `SET LOCAL ROLE app_api` so RLS actually applies (the
-- pool's session role is normally a superuser which RLS bypasses).

ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;

CREATE POLICY audit_events_tenant_isolation ON audit_events
    FOR SELECT
    TO app_api
    USING (
        -- NULLIF + ::uuid avoids both:
        --   * `''::uuid` errors when the GUC is set to an empty string,
        --   * non-NULL false matches when the GUC is unset (NULLIF + cast
        --     gives NULL; `chain_id = NULL` is NULL → USING returns false).
        chain_kind = 'tenant'
        AND chain_id = NULLIF(current_setting('app.current_tenant', true), '')::uuid
    );
