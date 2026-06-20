-- tenant_dek_wrappings — per-(tenant, record_kind, version) wrapped DEK
-- store per ADR-023 §A. Each row is one DEK; the wrapped bytes are
-- AEAD-encrypted under the master KEK via flight-academy-store's
-- `MasterKek::wrap`. Crypto-shred = DELETE row (per ADR-001 §D's GDPR
-- Art. 17 mechanism).
--
-- Why no RLS at this layer: tenant_dek_wrappings is touched at
-- controller-creation time, before any `app.current_tenant` GUC has
-- been set in the request lifecycle (per ADR-001 §C). The KeyProvider
-- impl passes tenant_id explicitly in every WHERE clause; the
-- structural isolation comes from the ON DELETE CASCADE from tenants
-- plus the partial unique index on the active state. A future RLS
-- pass could attach if request paths emerge that read DEK wrappings
-- in a tenant-scoped transaction.
--
-- Why no user_dek_wrappings counterpart yet: the `users` table lands
-- with Slice D auth. Until then, SqlxKeyProvider returns a structured
-- error for User controllers, and ADR-023 §A's invariant — "erasing
-- a tenant cannot affect any user's encrypted records" — holds
-- trivially because no user-level encrypted data exists yet.

CREATE TABLE tenant_dek_wrappings (
    tenant_id     uuid     NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    -- Free-form text rather than a Postgres enum so adding a new
    -- record_kind (e.g. 'safety' when ADR-001 §G safety occurrences
    -- land, 'medical' when sensitive PII columns join in Slice G) is
    -- an INSERT, not an `ALTER TYPE ... ADD VALUE` migration that
    -- would force the ADR-003 §A non-transactional exception. Bounded
    -- length so a malformed caller can't fill the column with
    -- megabytes of text.
    record_kind   text     NOT NULL CHECK (length(record_kind) BETWEEN 1 AND 64),
    dek_version   integer  NOT NULL CHECK (dek_version >= 1),
    -- The AEAD-encrypted DEK + nonce: 12-byte nonce || 32-byte
    -- ciphertext || 16-byte tag = 60 bytes for AES-256-GCM-SIV at
    -- v0.1. Bounded generously (max 1024) to admit alternate wrap
    -- algorithms (different nonce sizes; future algorithm rolls
    -- per ADR-022 §F) without a schema change.
    wrapped_bytes bytea    NOT NULL CHECK (octet_length(wrapped_bytes) BETWEEN 32 AND 1024),
    -- algo_id of the cipher that wrapped this DEK. 0x01 = AES-256-GCM-SIV
    -- at v0.1 per ADR-022 §A. smallint (i16) accommodates the full u8
    -- range without sign issues. Stored as the wrap-layer algorithm
    -- rather than carried in `wrapped_bytes` so a KEK-algorithm
    -- rotation (e.g. ML-KEM hybrid per ADR-013 §I) is a row-rewrap
    -- migration that doesn't touch the data ciphertexts.
    wrap_algo_id  smallint NOT NULL CHECK (wrap_algo_id BETWEEN 1 AND 254),
    -- Identifier of the KEK that wrapped this DEK. At v0.1 with an
    -- in-process master key this is the constant 'master:v1'; AWS KMS
    -- or OpenBao Transit would record the upstream key ARN or Vault
    -- key path here. KEK rotation per ADR-023 §E3 increments the :vN
    -- suffix; multiple kek_id values coexist mid-rotation.
    kek_id        text     NOT NULL CHECK (length(kek_id) BETWEEN 1 AND 200),
    state         text     NOT NULL CHECK (state IN ('active', 'retired')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    retired_at    timestamptz,
    PRIMARY KEY (tenant_id, record_kind, dek_version),
    -- State invariant: an active row has no retired_at; a retired
    -- row has one. Rules out the mid-state "marked retired but no
    -- timestamp" that an in-flight UPDATE could otherwise leave.
    CONSTRAINT tenant_dek_wrappings_state_consistency CHECK (
        (state = 'active'  AND retired_at IS NULL)
        OR
        (state = 'retired' AND retired_at IS NOT NULL)
    )
);

-- Exactly one active row per (tenant, record_kind) per ADR-023 §A.
-- Partial unique index lets retired versions accumulate (they stay
-- readable until shredded by DELETE) while preventing a generation
-- from being layered on top of an existing active row.
CREATE UNIQUE INDEX tenant_dek_wrappings_one_active
    ON tenant_dek_wrappings (tenant_id, record_kind)
    WHERE state = 'active';

-- Grants. SqlxKeyProvider runs as app_api with full DML — INSERT for
-- generation, UPDATE for rotation's retirement step, DELETE for the
-- crypto-shred ceremony per ADR-013 §H. The "app role can DELETE its
-- own wrappings" shape enables emergency shredding (ADR-023 §E2)
-- without escalating to app_migrator. app_migrator already owns the
-- table via the ROLE the migration runs as.
GRANT SELECT, INSERT, UPDATE, DELETE ON tenant_dek_wrappings TO app_api;
