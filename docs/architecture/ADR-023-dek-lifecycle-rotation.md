# ADR-023 — DEK lifecycle, versioning, and rotation

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-19 |
| **Deciders** | @ICreateThunder |
| **Tags** | crypto, encryption, dek, kek, rotation, crypto-shred, gdpr |
| **Supersedes** | (none — refines [ADR-001 §D](ADR-001-platform.md) and [ADR-022 §B](ADR-022-pluggable-aead.md)) |

## Context

[ADR-001 §D](ADR-001-platform.md) decided envelope encryption with a per-tenant
or per-user Data Encryption Key (DEK), wrapped by a Key Encryption Key (KEK),
with the wrapped DEK stored "alongside the owning entity" — concretely named
`tenants.dek_wrapped` and `users.dek_wrapped`. [ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md)
added the *controller-owner rule*: the DEK is the owning controller's, looked
up via `KeyProvider::for_record(record_kind, controller)`. [ADR-001 §G](ADR-001-platform.md)
further required a **separate per-tenant safety key** distinct from the general
tenant DEK. [ADR-022 §B](ADR-022-pluggable-aead.md) specified the ciphertext
envelope format `[version|algo_id|nonce_len][nonce][ciphertext||tag]` with a
per-ciphertext algorithm tag for algorithm agility.

[Slice C2a (PR #35, 98e3df9)](https://github.com/flight-academy-uk/flight-academy/pull/35)
landed a first `KeyProvider` implementation using HKDF-SHA256 derivation:

```text
DEK = HKDF(salt = controller.kind_prefix || uuid,
           ikm  = master_kek,
           info = record_kind)
```

That mechanism is **structurally incompatible with ADR-001 §D's crypto-shred
property.** ADR-001 §D's [GDPR Art. 17](https://gdpr-info.eu/art-17-gdpr/)
guarantee is: *deleting* the wrapped DEK renders every encrypted column under
it mathematically unrecoverable. HKDF derivation has no wrapped DEK to delete —
the DEK is recomputed on every call from `(master, controller_id, record_kind)`.
The salt is the public controller identity (kind byte + UUID); deleting it is
impossible because [ADR-007 §C](ADR-007-sync-filtering-deletion.md) requires
the controller row to persist as a tombstone. Destroying the master KEK is
catastrophic — it shreds *every* controller's data simultaneously.

Three further forces meet at this decision point:

1. **Rotation is a stated capability.** [ADR-001 §D](ADR-001-platform.md)'s
   comparison table lists "rotate master key → rewrap N DEKs, ciphertext
   untouched" as a property of envelope encryption. The current shape provides
   no DEK to rewrap. [ADR-013 §F](ADR-013-auth-keys.md) sets the rotation
   precedent for per-controller artefact keys (annual; 90-day overlap; ≤1-hour
   emergency response). The encryption surface should match that posture.
2. **Breach response must be tractable.** A leaked DEK should be rotatable
   without destroying the controller's data. The mechanism must support both
   periodic best-practice rotation and emergency response within the same
   model.
3. **Multiple record_kinds per controller.** [ADR-001 §G](ADR-001-platform.md)
   already requires a separate safety key per tenant; future record_kinds
   (e.g. distinct keys per data-class tier per
   [ADR-008 §B](ADR-008-data-sharing-posture.md)) will follow. A single
   `dek_wrapped` column does not model this.

Constraints inherited from related ADRs:

- [ADR-001 §D](ADR-001-platform.md) — crypto-shred property is load-bearing for
  GDPR Art. 17 erasure
- [ADR-003 §B/§F](ADR-003-db-migrations.md) — expand-contract migration
  discipline; chunked sweep for large data rewrites
- [ADR-007 §C/§D](ADR-007-sync-filtering-deletion.md) — soft-delete tombstones
  for operational delete; crypto-shred for statutory erasure; `resource.erased`
  event class
- [ADR-009 §C](ADR-009-event-streams-and-retention.md) — per-tenant audit
  chain; key-lifecycle events appear here
- [ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md) — controller-owner rule;
  `KeyProvider::for_record(record_kind, controller)` API
- [ADR-013 §F](ADR-013-auth-keys.md) — rotation cadence precedent
- [ADR-013 §H](ADR-013-auth-keys.md) — DEK destruction "requires confirmation
  ceremony and platform-chain entry"
- [ADR-022 §B/§F](ADR-022-pluggable-aead.md) — envelope format; sweep job for
  algorithm migration (same shape applies to DEK rotation)

What we do not yet know: how often operators will choose to rotate (annual is
the calendar default; operator override permitted); how large encrypted
columns will grow before sweep cost becomes load-bearing (drives the
2-layer-vs-3-layer decision in §D); whether self-host operators will adopt the
versioned-DEK shape uniformly or want a "single static DEK forever" mode (we
do not offer that mode at v0.1; rotation discipline is universal).

## Decision

**A wrapped DEK exists per `(controller, record_kind, version)` triple, stored
in `tenant_dek_wrappings` / `user_dek_wrappings` tables. The ciphertext
envelope carries the DEK version it was written under. Rotation generates a new
version atomically, retires the previous, and runs a chunked sweep that
re-encrypts data under the new version before the retired wrapping is shredded
by row DELETE.**

ADR-001 §D's single-`dek_wrapped`-column shape is replaced by this versioned
table-per-controller-kind shape; ADR-001 §D's envelope-encryption *posture*
(per-controller DEK + KEK + crypto-shred for Art. 17) is unchanged. ADR-022 §B's
envelope format bumps from `0x01` to `0x02` to carry the DEK version.

### A. Storage shape — two tables, FK to owner with ON DELETE CASCADE

```sql
CREATE TABLE tenant_dek_wrappings (
    tenant_id      uuid     NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    record_kind    text     NOT NULL,
    dek_version    int4     NOT NULL,
    wrapped_bytes  bytea    NOT NULL,
    wrap_algo_id   smallint NOT NULL,  -- algo_id of the cipher that wrapped this DEK
    kek_id         text     NOT NULL,  -- identifier of the KEK that wrapped this DEK
    state          text     NOT NULL CHECK (state IN ('active', 'retired')),
    created_at     timestamptz NOT NULL DEFAULT now(),
    retired_at     timestamptz,
    PRIMARY KEY (tenant_id, record_kind, dek_version)
);

CREATE UNIQUE INDEX one_active_per_tenant_record_kind
    ON tenant_dek_wrappings (tenant_id, record_kind)
    WHERE state = 'active';

-- analogous shape for user_dek_wrappings(user_id, ...)
```

Two tables rather than one polymorphic table — each has a real foreign key to
its owner. `ON DELETE CASCADE` from the owner row means the erasure ceremony
([ADR-013 §H](ADR-013-auth-keys.md): confirmation + platform-chain entry) is
implemented as a single `DELETE FROM tenants WHERE id = $1` inside a
transaction; the cascade shreds every DEK row for that tenant atomically.
[ADR-007 §C](ADR-007-sync-filtering-deletion.md) soft-delete tombstones live on
the owner row's `deleted_at`; statutory erasure is the hard `DELETE` that fires
the cascade.

**Cross-controller isolation invariant.** The two-table shape *structurally*
enforces [ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md)'s controller-owner
rule: **erasing a tenant cannot affect any user's encrypted records, and
erasing a user cannot affect any tenant's encrypted records.** The
`ON DELETE CASCADE` path from `tenants` reaches only `tenant_dek_wrappings`;
the path from `users` reaches only `user_dek_wrappings`. There is no SQL
operation — accidental, malicious, or buggy — that can shred the wrong
controller's keys. A pilot who is a member of multiple tenants per
[ADR-001 §A](ADR-001-platform.md) keeps every personal record (logbook,
medical metadata, passport, ratings) decryptable under their own DEK no matter
how many of those tenants exercise erasure; references in the pilot's records
to erased tenants become dangling pseudonyms per
[ADR-012 §C/§E1](ADR-012-cross-tenant-dek-erasure.md) without affecting the
encrypted content. This is the schema-level expression of the privacy
promise — not an application-level convention that could be forgotten.

`record_kind` is `text`, not a Postgres enum. Adding a new record_kind
(e.g. `"safety"` when ADR-001 §G safety occurrences land; `"medical"` when
sensitive PII columns land in Slice G) is then an INSERT, not an
`ALTER TYPE ... ADD VALUE` migration — which would require the
[ADR-003 §A](ADR-003-db-migrations.md) non-transactional exception. At v0.1
the only record_kind written is `"default"`.

`wrap_algo_id` records which AEAD wrapped the DEK; per
[ADR-022 §A](ADR-022-pluggable-aead.md) this is `0x01` (AES-256-GCM-SIV) at
v0.1. Storing it makes the wrap layer self-describing and lets a future KEK
algorithm change (e.g. ML-KEM hybrid per [ADR-013 §I](ADR-013-auth-keys.md))
run as a row-rewrap migration without touching ciphertexts.

`kek_id` identifies the KEK that wrapped this DEK. At v0.1 with a single
in-process master key it is the constant string `"master:v1"`; when AWS KMS or
OpenBao Transit land, it is the upstream key ARN or Vault key path. KEK
rotation (§E) increments the `:vN` suffix; multiple `kek_id` values coexist
during rotation.

### B. Envelope format bump — version `0x02` carries `dek_version`

```text
+---------+----------+---------------+------------+-----------------+-----------+
| version | algo_id  | dek_version   | nonce_len  | nonce           | cipher+tag|
| 1 byte  | 1 byte   | 4 bytes (BE)  | 1 byte     | nonce_len bytes | variable  |
+---------+----------+---------------+------------+-----------------+-----------+
```

`ENVELOPE_VERSION` increments from `0x01` to `0x02`. `dek_version` is a
big-endian `u32`. The AAD per
[ADR-022 §C](ADR-022-pluggable-aead.md) extends to include the four
`dek_version` bytes: AAD = `[version|algo_id|dek_version|nonce_len]` +
`record_kind:record_id:column`. The AAD change means a `dek_version` swap is
an authentication failure, not silent misdispatch.

Reads parse `dek_version` from the header before deciding which wrapping row
to unwrap. The DEK lookup is by `(controller, record_kind, dek_version)`; if
no row matches, the read fails with `KeyMaterialUnavailable` —
distinguishable in audit from a tag-verification failure.

**Hard cut, not dual-version.** No production data exists under
`ENVELOPE_VERSION = 0x01` at v0.1 — ADR-022 was Accepted on 2026-06-18 and
no encrypted columns ship yet. The `0x01` format is retired; `Envelope::decrypt`
rejects it with an explicit "legacy envelope format" error. A future format
bump (e.g. AAD shape change) would carry dual-version support; this one does
not.

`u32` for `dek_version` allows ~4 billion rotations per
`(controller, record_kind)`. At annual rotation that is 4 billion years —
ample. `u16` was considered (§Alternatives) and rejected as too narrow given
emergency-rotation scenarios.

### C. Generation lifecycle — eager at controller create

Every tenant row created inserts a `tenant_dek_wrappings` row with
`record_kind = 'default'`, `dek_version = 1`, `state = 'active'`. Every user
row analogously creates `user_dek_wrappings` with `record_kind = 'default'`.
Additional record_kinds (e.g. `safety` for tenants when ADR-001 §G lands) are
added by INSERT in the migration that introduces the corresponding feature.

This mirrors [ADR-013 §C](ADR-013-auth-keys.md)'s eager artefact-key
generation: the invariant becomes "every controller has at least one active
DEK per record_kind," and double-checked-locking on first write is eliminated.

Generation steps inside a single transaction:

1. Draw 32 random bytes from `OsRng` per
   [ADR-022 §G](ADR-022-pluggable-aead.md).
2. Wrap with the active KEK using the configured AEAD (AES-256-GCM-SIV at
   v0.1): the wrap operation reuses the same `AeadCipher` infrastructure as
   data encryption, with the master KEK as the cipher key and the DEK bytes
   as the plaintext.
3. AAD for the wrap: `"dek-wrap:" || controller_kind || ":" || controller_id || ":" || record_kind || ":" || dek_version`.
   The `"dek-wrap:"` prefix prevents an attacker who obtains a wrapped DEK
   from substituting it as a data-column ciphertext (different AAD prefix
   fails authentication).
4. INSERT the wrapping row with `state = 'active'`.

The DEK plaintext is zeroized after wrapping; only the wrapped bytes persist.

### D. Two-layer envelope (KEK → DEK); three-layer deferred

At v0.1 the envelope is two layers: data ciphertext is encrypted directly
under the DEK; the DEK is wrapped under the KEK. Rotation of a DEK therefore
requires re-encrypting every data row that referenced the old DEK — the
[ADR-022 §F](ADR-022-pluggable-aead.md) sweep job pattern (§F below).

A **three-layer** alternative — per-row Data Key (DK) wrapped under the DEK,
data encrypted under the DK — was considered. Three-layer reduces rotation
cost from "re-encrypt the ciphertext (variable size)" to "rewrap the DK
(32 bytes)" per row, but it still requires writing to every row, adds a per-
row wrapped-DK column to every encrypted column, complicates the AAD binding
(must bind ciphertext to its wrapping DK), and roughly doubles read latency
(two unwraps instead of one). At v0.1 column sizes the saving is theoretical;
the complexity is real. **Three-layer is deferred to a future ADR if and when
encrypted-column sizes or rotation frequency make the sweep cost
load-bearing**; the trait shape of §G is forward-compatible (a 3-layer
implementation could be a new `KeyProvider` impl behind the same trait).

### E. Rotation events — three classes

**E1 — Routine DEK rotation** (per `(controller, record_kind)`):

1. Operator triggers, or scheduled job fires (annual default per
   [ADR-013 §F](ADR-013-auth-keys.md)).
2. New DEK generated (§C steps 1–4) with `dek_version = max + 1`,
   `state = 'active'`.
3. Old active row's `state` becomes `'retired'`, `retired_at = now()`.
4. The transaction emits a `key.rotated` audit event per
   [ADR-009 §C](ADR-009-event-streams-and-retention.md) with the controller
   pseudonym, record_kind, old version, new version. No PII per
   [ADR-004 §D](ADR-004-defence-in-depth.md).
5. New writes use the new active version; reads under the old version still
   decrypt because the retired wrapping row still exists.
6. The sweep job (§F) re-encrypts data rows from the retired version to the
   new active version over time.
7. After the sweep completes for the retired version AND the 90-day overlap
   window elapses (mirroring [ADR-013 §F](ADR-013-auth-keys.md) artefact-key
   policy), the retired row is hard-deleted. Audit event: `key.shredded`. The
   retired DEK is now unrecoverable; any orphan ciphertexts (e.g. forgotten
   in a backup table not covered by the sweep) become permanently
   unreadable — this is the intended terminal state, not a bug.

**E2 — Emergency DEK rotation** (breach response):

Steps 1–4 identical. The 90-day overlap is collapsed: the operator may
DELETE the retired row immediately after the sweep verifies no remaining
references (or sooner, accepting that any unreencrypted ciphertexts become
unreadable — typically the correct trade in a confirmed-leak scenario).
Audit chain records the elevated reason and operator identity per
[ADR-010 §E](ADR-010-platform-operator-access.md). Target end-to-end time:
≤ 1 hour to step 4 (the wrapping is rotated; sweep continues in
background), same as [ADR-013 §F](ADR-013-auth-keys.md) session-key
emergency response.

**E3 — KEK rotation** (cheap; data untouched):

A new KEK is provisioned with a new `kek_id`. For each row in both DEK-
wrapping tables: unwrap with the prior KEK, rewrap with the new KEK, UPDATE
`wrapped_bytes` and `kek_id`. Data ciphertexts are not touched. The
operation is chunked per [ADR-003 §F](ADR-003-db-migrations.md). Old KEK is
destroyed only after every wrapping row references the new KEK and the
audit-chain verifier confirms — this is the ADR-001 §D "rotate master key →
rewrap N DEKs, ciphertext untouched" property realised. KEK rotation does
not bump `dek_version`; the version space tracks the DEK only.

### F. Sweep job — chunked re-encryption under ADR-003 §F discipline

The sweep job re-encrypts data rows from a retired DEK version to the
active version. It follows [ADR-003 §F](ADR-003-db-migrations.md) chunking:

- Bounded batches by primary-key range or `LIMIT`-ed `UPDATE ... WHERE id IN (...)`.
- Per-batch commit and brief pause between batches; throttle observable
  via metrics so an operator can speed it up for emergency response.
- Idempotent: a row already at the active version is skipped (the envelope's
  `dek_version` header makes this a cheap byte check).
- Per-row operation: read ciphertext → parse envelope → unwrap retired DEK
  → decrypt → encrypt under active DEK → write back. Both unwraps and the
  decrypt/encrypt run inside the request-scoped DEK cache so the retired
  DEK is unwrapped at most once per worker generation.
- Progress recorded in a `dek_sweep_state` table per
  `(controller, record_kind, dek_version)`: rows-scanned, rows-rewritten,
  rows-skipped, last-cursor. Resumable.

The job is the same shape as
[ADR-022 §F](ADR-022-pluggable-aead.md)'s algorithm-migration sweep —
factored together in implementation. The implementation lands in a future
crate (likely `flight-academy-jobs` adjacent to `apps/api`); this ADR
documents the design.

### G. Trait surface — `KeyProvider`

> **Refined**: methods return `impl Future<Output = StoreResult<...>> + Send`
> rather than the bare `StoreResult<...>` shown below. The async return
> type accommodates DB-backed implementations (e.g. the sqlx-backed
> `SqlxKeyProvider` in `flight-academy-db`) while costing nothing for
> sync implementations — `async fn` bodies with no `.await` are erased
> by Rust's async-fn-in-trait machinery. The trait's *responsibility*
> shape (the five methods, their parameters, their error surface) is
> unchanged; only the calling convention is.

The `KeyProvider` trait in `flight-academy-store`
([ADR-005 §C](ADR-005-workspace-layout.md)) takes the shape:

```rust
pub trait KeyProvider: Send + Sync + 'static {
    /// Generate, wrap, and store a new DEK for (controller, record_kind).
    /// Returns the new wrapping row's version. Used at controller create
    /// and at rotation.
    fn generate_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<u32>;

    /// Resolve the active DEK for (controller, record_kind). Used for
    /// writes — picks the row where state = 'active'.
    fn active_dek_for(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(Dek, u32)>;

    /// Resolve a specific DEK version for (controller, record_kind). Used
    /// for reads — looked up by the dek_version byte in the envelope.
    fn dek_at_version(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<Dek>;

    /// Rotate (E1/E2). Atomically generate new active + retire old.
    /// Returns (new_version, retired_version).
    fn rotate_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(u32, u32)>;

    /// Shred (DELETE) a retired wrapping row. Caller responsible for
    /// confirming the sweep is complete + overlap elapsed.
    fn shred_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<()>;
}
```

Two impls at v0.1:

- **In-memory** — backs unit tests; no schema needed. Holds wrapping rows in
  a `HashMap`.
- **sqlx-backed** — the production impl. Reads and writes
  `tenant_dek_wrappings` / `user_dek_wrappings`. Lands in C2b.3.

Future KMS-resident impls (AWS KMS, OpenBao Transit) wrap the existing
shape — the trait does not change.

### H. Audit chain integration

Every DEK lifecycle transition is an `audit_events` row per
[ADR-009 §C](ADR-009-event-streams-and-retention.md):

| Event | Fields | Plane |
| --- | --- | --- |
| `key.generated` | controller pseudonym, record_kind, version, kek_id | per-controller chain |
| `key.rotated` | controller pseudonym, record_kind, old_version, new_version, reason: routine\|emergency | per-controller chain |
| `key.shredded` | controller pseudonym, record_kind, version, sweep_complete: bool | per-controller chain |
| `kek.rotated` | old kek_id, new kek_id, rows_rewrapped | platform chain |
| `key.rotation.failed` | controller pseudonym, record_kind, reason | per-controller chain |

No PII per [ADR-004 §D](ADR-004-defence-in-depth.md) — opaque IDs only.
Audit emission is part of the rotation/shred transaction so audit and the
DEK row commit atomically.

### I. Self-host

A self-host install runs the tenant binary only
([ADR-013 §G](ADR-013-auth-keys.md)). Every decision above applies, with
two simplifications: the KEK is the operator-supplied master file (32 bytes
on disk per the current `KeyProvider::from_master_file` constructor); KEK
rotation is a manual operator ceremony rather than KMS-driven. The schema
tables are identical. Rotation discipline is identical. The sweep job runs
as part of the single binary's background task surface.

## Consequences

### Positive

- **Crypto-shred property is preserved literally.** Deleting a wrapping row
  shreds the DEK; data under that DEK becomes mathematically unrecoverable.
  GDPR Art. 17 has a real answer per ADR-001 §D's original guarantee.
- **Rotation is a small operation.** Per-controller DEK rotation is one
  INSERT plus one UPDATE plus an audit row — single transaction, ≤ ms.
  Sweep is a background concern, throttled at operator discretion.
- **KEK rotation is cheap.** Data untouched per ADR-001 §D's stated property.
  Per-DEK-row rewrap operation is O(N controllers × N record_kinds), not
  O(rows).
- **Breach response is tractable.** A leaked DEK can be rotated within an
  hour; the retired wrapping can be shredded out-of-band, severing the leak.
- **Per-record_kind separation is structural.** ADR-001 §G's safety key
  becomes a row with `record_kind = 'safety'` — no schema change.
- **Algorithm agility composes with key agility.** `wrap_algo_id` per
  wrapping row means a KEK algorithm change (e.g. ML-KEM hybrid per
  ADR-013 §I) is the same shape as a DEK rotation; the agility extends
  through the wrap layer.

### Negative

- **Two tables, multiplied rows.** Each tenant carries `N_record_kinds ×
  N_versions` rows; each user the same. At annual rotation with ~5 record
  kinds and 90-day overlap, ~6 rows per user — modest.
- **Envelope format bumps 0x01 → 0x02.** Per-ciphertext overhead increases
  by 4 bytes for `dek_version`. At hundreds of writes/day per tenant,
  irrelevant; on a 1TB encrypted column ~50MB. Accepted.
- **Sweep job is a new operational surface.** A background worker that
  rewrites encrypted data. Operator monitoring needed. Mitigated by reusing
  the [ADR-022 §F](ADR-022-pluggable-aead.md) sweep machinery — one
  implementation, two consumers.
- **Three-layer optimisation is foreclosed at v0.1.** If a future
  workload reveals sweep cost is unacceptably high, switching to 3-layer
  is itself a sweep migration. Accepted; the trait shape leaves the door
  open.
- **`record_kind` is text, not an enum.** A typo at INSERT time silently
  creates a new record_kind. Mitigated by an application-layer constants
  module and a CI lint that the inserted value is one of the registered
  constants.

### Neutral

- **`u32` for `dek_version`** — 4 bytes per ciphertext; allows annual
  rotation for 4 billion years. The width was chosen for emergency-
  rotation scenarios (multiple rotations per year if breach patterns
  emerge); `u16` would have been adequate for routine schedules but
  fragile under emergency operations.
- **ON DELETE CASCADE from owner row to DEK wrappings** — couples the
  erasure ceremony to a single `DELETE` on the owner row. The trade-off
  is that an accidental `DELETE` on a tenants row is catastrophic —
  mitigated by the [ADR-007 §C](ADR-007-sync-filtering-deletion.md)
  soft-delete-first discipline (hard DELETE only via the erasure
  ceremony per [ADR-013 §H](ADR-013-auth-keys.md)).
- **Audit emissions are inside the rotation transaction.** Ensures
  atomicity but bounds the transaction's external timing — audit
  insertion is fast (local DML) so this is acceptable.
- **OpenBao integration composes naturally** with the trait shape. A
  future `BaoKmsClient` impl of `KeyProvider` performs Transit-engine
  `encrypt`/`decrypt` for the wrap/unwrap; the on-disk envelope format
  is unchanged. Vault's native key rotation (`rewrap`) maps to §E3 KEK
  rotation; `rotate_dek` (§E1) is an application-level operation
  orthogonal to Vault key versions. (This is the same coexistence note
  ADR-022's §G Neutral makes for the AEAD side; whether to introduce
  the cluster-level OpenBao operator is tracked as a deferred
  infrastructure decision.)

## Alternatives considered

### Alternative — Keep HKDF derivation, store a per-controller "shred salt"

Add a `tenants.shred_salt bytea` column; derive `DEK = HKDF(salt =
shred_salt, ikm = master, info = record_kind)`. Erasure deletes the salt.
Rejected because the derivation is still mathematical, not envelope —
deleting the salt does shred the controller's data, but the *property* is
shred-by-deleting-a-secret-salt, not shred-by-deleting-a-wrapped-key.
Rotation has no natural shape (HKDF info-rotation is opaque; salt-rotation
shreds all prior data). Crypto-shred works; rotation doesn't. The two
properties are decision-coupled.

### Alternative — Single `dek_wrapped` column per ADR-001 §D as literally written

Keep the column; do not version; rotation = generate new DEK, sweep all
data, UPDATE the column. Rejected because in-flight reads under the old
DEK fail during the sweep window (the column has been overwritten); the
sweep is forced to run inside a transaction-equivalent lock window, which
is incompatible with [ADR-003 §F](ADR-003-db-migrations.md) chunking.
Versioning is the property that decouples sweep time from rotation time.

### Alternative — `dek_wrappings` JSONB column on the owner table

Store the array of wrapping rows as JSONB on `tenants` / `users` directly:
`tenants.dek_wrappings jsonb`. Rejected because per-row UPDATE on a JSONB
array on rotation is more expensive than a small INSERT into a child
table; because partial indexes (`WHERE state = 'active'`) are unavailable
on JSONB-array elements; because ON DELETE CASCADE is the natural shape
for the wrap rows to vanish with the owner. The child-table shape is
literally what RDBMSes are good at.

### Alternative — Single polymorphic table for both tenants and users

`controller_dek_wrappings (controller_kind, controller_id, ...)` with
controller_kind ∈ {'tenant', 'user'}. Rejected because the foreign-key
guarantee is lost — `controller_id` cannot reference two different tables.
A trigger could maintain the invariant, but a real FK is simpler and
catches more at INSERT time. Two tables cost one extra migration; the
deduplication is not worth losing referential integrity.

### Alternative — `u16` `dek_version`

Saves 2 bytes per ciphertext. Rejected because emergency-rotation
scenarios may require multiple rotations per year (re-emerging breach
patterns; supply-chain incidents). 65k rotations is enough for routine
operation but uncomfortable for incident-response scenarios where a
controller might rotate 5–10 times in a week. The 2-byte saving is not
load-bearing; the version headroom is.

### Alternative — Three-layer envelope (KEK → DEK → per-row DK) at v0.1

Per-row Data Key wrapped under the DEK; data encrypted under DK. DEK
rotation rewraps DKs only (no ciphertext touch). Rejected at v0.1
(considered for future) because: (a) doubles read latency (two unwraps);
(b) adds a per-row wrapped-DK column to every encrypted column,
multiplying storage; (c) the saving — re-writing a 32-byte DK instead of
a 50-byte JSON ciphertext — is theoretical at v0.1 column sizes; (d) the
sweep-cost argument has not been measured. Re-evaluate when an encrypted
column approaches MB range or rotation frequency materially exceeds
annual.

### Alternative — Rotate by appending to the wrapping column without versioning the envelope

Don't bump `ENVELOPE_VERSION`; instead, when decrypting, try the active
DEK; on failure, fall back to retired versions. Rejected because
"try each key" defeats AEAD's design (the tag failure is meant to be
distinguishable from corruption, not "try harder"); leaks via timing
which DEK version a ciphertext was under; complicates audit (failed
decrypt logs become noise). Carrying the version explicitly in the
envelope is the honest shape.

## References

### Related ADRs

- [ADR-001 §D](ADR-001-platform.md) — refined here: single-column shape
  replaced with versioned table-per-controller-kind; envelope-encryption
  posture (DEK + KEK + crypto-shred) unchanged.
- [ADR-003 §B/§F](ADR-003-db-migrations.md) — expand-contract migration
  discipline + chunked sweep; both apply to schema and rotation.
- [ADR-004 §D](ADR-004-defence-in-depth.md) — no-PII audit shape; opaque
  controller pseudonyms in key-lifecycle events.
- [ADR-007 §C/§D](ADR-007-sync-filtering-deletion.md) — soft-delete vs
  statutory erasure; `resource.erased` event lifecycle; ON DELETE CASCADE
  fires inside the erasure ceremony.
- [ADR-009 §C](ADR-009-event-streams-and-retention.md) — per-tenant audit
  chain for key.* events.
- [ADR-010 §E](ADR-010-platform-operator-access.md) — platform-chain entry
  for KEK rotations and emergency operations.
- [ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md) — controller-owner rule
  and `KeyProvider::for_record(record_kind, controller)`; the trait surface
  in §G implements this rule literally.
- [ADR-013 §C/§F/§H/§I](ADR-013-auth-keys.md) — eager generation
  precedent; annual rotation cadence; confirmation ceremony for key
  destruction; PQ-readiness via cipher-suite agility.
- [ADR-022 §A/§B/§C/§F](ADR-022-pluggable-aead.md) — algo_id dispatch;
  envelope format bumped here; AAD composition extended for dek_version;
  sweep-job pattern reused.

### Project documents

- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 28 (truth —
  the wrapping is a real key, not a derivation pretending to be one), 38
  (diligence — version space and audit events designed up-front rather
  than retrofitted after a breach), 48 (watchfulness — rotation is a
  scheduled discipline, not a reaction).

### External standards

- GDPR Article 17 — Right to Erasure: <https://gdpr-info.eu/art-17-gdpr/>
- NIST SP 800-57 Part 1 Rev 5 — Recommendation for Key Management,
  §8.2.4 (cryptoperiods), §8.3.5 (rotation): <https://csrc.nist.gov/publications/detail/sp/800-57-part-1/rev-5/final>
- NIST SP 800-152 — Profile for U.S. Federal Cryptographic Key Management:
  <https://csrc.nist.gov/publications/detail/sp/800-152/final>
- HashiCorp Vault Transit engine — `rewrap`:
  <https://developer.hashicorp.com/vault/api-docs/secret/transit#rewrap-data>

## Notes

The most reversible part of this ADR is the v0.1 two-layer envelope
decision (§D). If sweep cost becomes load-bearing, switching to three-
layer is itself a sweep — chunked, throttled, audited. The trait shape
in §G abstracts the layering decision; a three-layer KeyProvider impl
implements the same five methods, the wrappers and Envelope code do
not change.

The least reversible part is the controller-owner FK structure (§A).
Switching to a polymorphic single table would require dropping the FK
constraints and reconstituting referential integrity in application
code — destructive, irreversible, expand-contract over months. Locking
the two-table shape now is the load-bearing choice.

C2a (PR #35, 98e3df9) is replaced by the C2b series. The C2a code is not
production data — no encrypted column ships under the HKDF mechanism —
so the migration is a code change with no data-rewrite. The replacement
PRs (C2b.1 through C2b.4) land in sequence; C2c bundles with the first
real sensitive column.
