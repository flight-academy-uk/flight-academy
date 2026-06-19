# ADR-022 — Pluggable AEAD selection — default AES-256-GCM-SIV

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-18 |
| **Deciders** | @ICreateThunder |
| **Tags** | crypto, encryption, aead, supply-chain |
| **Supersedes** | (none — refines [ADR-001 §D](ADR-001-platform.md)) |

## Context

[ADR-001 §D](ADR-001-platform.md) specified AES-256-GCM as the column-level AEAD for envelope encryption. Three forces push us to broaden that choice as Slice C2 lands:

1. **Nonce discipline.** AES-256-GCM's 96-bit nonce must be unique per key for the lifetime of that key — reuse is catastrophic (authentication-key recovery via GHASH polynomial extraction, enabling forgery of any message under the key, plus plaintext disclosure of the two colliding messages). The discipline is tractable but is one more invariant the application must defend, and a per-row encrypted column generates many writes per key. AES-256-GCM-SIV (RFC 8452) is nonce-misuse-resistant — accidental reuse leaks only "these two plaintexts were equal," not the key. The roughly 10% performance cost is negligible at our write volume.

2. **Software performance on diverse architectures.** AES-256-GCM is hardware-accelerated where AES-NI is available (Hetzner-class x86_64; recent ARMv8 with crypto extensions). Self-host operators may deploy on older ARM, RISC-V, or other architectures without crypto acceleration. ChaCha20-Poly1305 is software-fast everywhere — independent of CPU crypto extensions.

3. **Algorithm agility.** Per ADR-013 §J cipher-suite agility is already a load-bearing principle for FA's signing surface. Extending the same agility to the encryption surface is a small architectural cost paid now for substantial migration flexibility later (post-quantum, regulator-driven swaps, hardware deprecation).

The agility property must be present in the **ciphertext format**, not just the application configuration. A column may carry ciphertexts under multiple algorithms simultaneously during migration; reads must dispatch on a per-ciphertext algorithm tag without operator coordination.

Forces: defense-in-depth on the nonce-discipline category ([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instrument 48 — watchfulness); self-host parity across architectures (35–36 — restraint on hardware assumptions); honesty about the migration cost of locking a single algorithm forever (24 — no pretence that one cipher fits all futures); supply-chain hygiene — every algorithm shipped is one more audited dependency to defend (35–36 again).

## Decision

**Three AEAD algorithms ship in `flight-academy-store`, each implementing a common `AeadCipher` trait, dispatched by a per-ciphertext algorithm-tag byte. Default for new writes is AES-256-GCM-SIV; ChaCha20-Poly1305 and AES-256-GCM also ship for operator selection and forward migration. New algorithms join via trait impl and a registered algorithm ID — no schema or wire-format change.**

ADR-001 §D's specification of AES-256-GCM reads through this ADR as one of three shipped algorithms; not the default; still in force for compatibility with any ciphertext written under it.

### A. The shipped set

| Algorithm | Algo ID | Crate | Nonce | Default for new writes |
| --- | --- | --- | --- | --- |
| **AES-256-GCM-SIV** (RFC 8452) | `0x01` | `aes-gcm-siv` (RustCrypto) | 96-bit | **Yes** |
| **ChaCha20-Poly1305** (RFC 8439) | `0x02` | `chacha20poly1305` (RustCrypto) | 96-bit | No |
| **AES-256-GCM** | `0x03` | `aes-gcm` (RustCrypto) | 96-bit | No |
| (reserved for XChaCha20-Poly1305) | `0x04` | — | 192-bit | (future) |
| (reserved for AEGIS-256) | `0x05` | — | 256-bit | (future) |
| (reserved for Ascon-128a) | `0x06` | — | 128-bit | (future) |
| (further reserved) | `0x07–0xFE` | — | — | (future) |
| (reserved as "do-not-use" sentinel) | `0xFF` | — | — | never assignable |

All three v0.1 algorithms use 256-bit keys and produce 128-bit authentication tags. Same key size means a DEK derived for one algorithm is structurally usable with any algorithm in the set; in practice keys are bound to an algorithm by the ciphertext header and we do not re-key across algorithms within a single deployment.

`0x00` is reserved as a "format-error" sentinel (never assigned to an algorithm) so a zero-byte read is unambiguous corruption rather than a valid algorithm.

### B. Ciphertext envelope format

> **Refined by [ADR-023 §B](ADR-023-dek-lifecycle-rotation.md)** — `ENVELOPE_VERSION` bumps from `0x01` to `0x02` to carry a 4-byte `dek_version` (big-endian `u32`) after `algo_id`. AAD per §C extends to include the four `dek_version` bytes. The §A `algo_id` dispatch and per-ciphertext self-describing header property are unchanged. No production data exists under `0x01`; the bump is a hard cut.

Every encrypted value carries a self-describing header so reads dispatch the right algorithm without out-of-band coordination.

```
+---------+----------+------------+-----------------+------------------------+
| version | algo_id  | nonce_len  | nonce           | ciphertext || auth_tag |
| 1 byte  | 1 byte   | 1 byte     | nonce_len bytes | variable               |
+---------+----------+------------+-----------------+------------------------+
```

- **`version`** — format version. `0x01` at v0.1. Reserved space for future format changes (e.g. additional metadata fields, AAD-binding shape changes).
- **`algo_id`** — algorithm tag per §A.
- **`nonce_len`** — explicit nonce length to support algorithms with different nonce sizes (current set is all 12; the reserved range includes 16/24/32). Bounded to `[12, 32]` by validation; values outside this range are format errors.
- **`nonce`** — raw nonce bytes for the named algorithm.
- **`ciphertext || auth_tag`** — algorithm output (the AEAD impl handles tag placement internally; we treat the suffix as opaque).

The header is not inside the AEAD ciphertext, but its bytes are bound to the AEAD via the AAD parameter (§C), so any header tampering causes tag verification to fail.

The format is binary, not hex/base64-encoded — it lives in `bytea` columns and serialises as `\x…` for psql display. Wrapper types (`EncryptedString`, `EncryptedJson`) handle the bytea round-trip transparently.

### C. AAD binding

Each encryption call passes an Additional Authenticated Data (AAD) value: the **column-record identity** — concretely `record_kind || ":" || record_id || ":" || column_name` as bytes. This binds a ciphertext to its position in the schema, so:

- An attacker who swaps a ciphertext between rows (e.g. moving a `medical_certificate_number` ciphertext from user A's row to user B's row) fails tag verification.
- A column rename without a re-encryption migration fails decryption — surfacing the schema change as a CI test failure rather than silent corruption.

The header bytes (version + algo_id + nonce_len) are also folded into AAD so a header-swap attack fails verification.

### D. Trait shape

```rust
pub trait AeadCipher: Send + Sync + 'static {
    fn algo_id(&self) -> u8;
    fn key_size(&self) -> usize;   // always 32 for the v0.1 set
    fn nonce_size(&self) -> usize; // always 12 for the v0.1 set
    fn encrypt(&self, key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8])
        -> Result<Vec<u8>, AeadError>;
    fn decrypt(&self, key: &[u8], nonce: &[u8], aad: &[u8], ciphertext: &[u8])
        -> Result<Vec<u8>, AeadError>;
}
```

A `CipherRegistry` maps `algo_id → Box<dyn AeadCipher>`. Decrypt operations read the algo_id from the header, look up the impl, and dispatch. Encrypt operations pick the configured default (or per-call override) and write the impl's `algo_id` into the header.

### E. Default selection and per-deployment override

The default for new writes is AES-256-GCM-SIV. Operators may override per-deployment via configuration:

```text
FA_DEFAULT_AEAD = "aes256-gcm-siv"   (default; if unset)
              | "chacha20-poly1305"  (software-only environments)
              | "aes256-gcm"         (compatibility with prior writes)
```

The override applies only to new writes. Existing ciphertexts continue to decrypt under their original algorithm by header dispatch — no migration is forced by an override change.

### F. Forward migration

A database may simultaneously contain ciphertexts under multiple algorithms (one per row), enabling:

- **Operator-initiated migration**: switch the default; new writes use the new algorithm; old reads still work; an optional sweep job re-encrypts old rows on a throttled cadence under [ADR-003 §F](ADR-003-db-migrations.md) chunking discipline.
- **Algorithm deprecation**: when a v0.1 algorithm is removed (e.g. post-quantum mandate retires AES-256-GCM), the sweep job runs before the impl is removed; CI verifies no remaining ciphertext carries the deprecated tag.
- **New algorithm rollout**: a new impl ships, claims a reserved algo_id, becomes available for operator override; existing data unaffected.

Migration is data movement, not schema change — it does not interact with [ADR-003 §B](ADR-003-db-migrations.md) expand/contract.

### G. CSPRNG source for nonces

All nonces are drawn from `OsRng` (the `rand_core::OsRng` interface to the OS CSPRNG). No nonce counter state is held by the application; each encrypt call draws fresh nonce bytes. GCM-SIV's nonce-misuse resistance means even a CSPRNG fault is bounded; the other two algorithms rely on CSPRNG quality for nonce uniqueness (96-bit random nonce collision probability is negligible at our write volume — `2^48` writes per key before practical concern).

## Consequences

### Positive

- **Nonce-misuse defense in depth.** The default (GCM-SIV) is the only algorithm in the set where accidental nonce reuse does not catastrophically leak the key. Worst-case failure mode is bounded.
- **Self-host architectural portability.** ChaCha20-Poly1305 gives software-fast encryption on ARM-without-crypto-extensions, RISC-V, and other architectures.
- **Forward migration without out-of-band coordination.** A single bytea column can carry ciphertexts under multiple algorithms; reads dispatch by header byte. Migration is a sweep job, not a schema change.
- **Algorithm agility is structurally present.** Adding a new algorithm is a trait impl + algo_id claim + registry registration. No schema, wire-format, or application code outside the cipher module changes.
- **Honest schema-binding.** The AAD `record_kind:record_id:column` binding turns ciphertext-swap attacks into authentication failures and surfaces unsafe column renames at CI time.
- **Per-ciphertext self-describing format.** A row dumped to a backup carries its algorithm with it; no decoder-side state required to interpret.

### Negative

- **Three audited crypto dependencies** to track. `aes-gcm-siv` is the youngest of the three (less battle-tested than `aes-gcm` and `chacha20poly1305`). Mitigated by tagging the algorithm so we can retire it via the sweep job in §F if a vulnerability is disclosed.
- **Per-deployment configuration surface.** Operators choose a default and may get it wrong. Mitigation: default is sane (GCM-SIV); explicit error if the env var is set to an unknown value; documented operational guidance.
- **Format-version field is overhead** (1 byte per ciphertext). At hundreds of writes/day on small column values, irrelevant; on a 1TB-encrypted-column database, ~50MB. Accepted.
- **Header is not itself MACed.** Bound to AEAD via AAD, but a header-byte flip results in decryption failure rather than authenticated rejection. Indistinguishable at the application layer (both raise `AeadError::Decrypt`), but a low-level corruption analyst will see "decryption attempted with algo X, failed" rather than "header tampering detected." Acceptable.

### Neutral

- **All v0.1 algorithms use 12-byte nonces** — `nonce_len` field is always `0x0C` in v0.1 ciphertexts. The field is paid for now to avoid a format-version bump when XChaCha20-Poly1305 (24-byte nonce) joins.
- **`0xFF` is never assignable as an algo_id** — reserved to make zero-padded reads of an all-`0xFF` buffer (a common allocation default) immediately recognisable as corruption.
- **No per-algorithm tunables exposed.** The default tag length is 128 bits for all three; AAD shape is fixed; nonce size is fixed per algorithm. Tunables would be agility theatre — they multiply the configuration matrix without producing distinct security postures we'd actually want.
- **OpenBao integration (per [[openbao-cluster-queue]]) coexists.** The `AeadCipher` trait describes the AEAD primitive; the `KeyProvider` trait (separate; ADR-001 §D / ADR-012 §A) describes the DEK source. A future `BaoKmsClient` impl of `KeyProvider` does Transit-engine `encrypt`/`decrypt` against Vault; the on-disk ciphertext format is unchanged.

## Alternatives considered

### Alternative — Stay with AES-256-GCM only (per ADR-001 §D as written)

Single algorithm; simplest possible code; least supply-chain surface. Rejected because the nonce-discipline burden is high for a column-encryption use case (one DEK encrypts many small values, each needing a unique nonce), because self-host on non-AES-NI architectures pays a real performance cost, and because algorithm agility is a known requirement for any encryption surface with a 5–10 year horizon. The ADR-013 §J precedent already applies the same principle to signing. We would reverse this only if the supply-chain cost of three impls vs one proved load-bearing in practice — `aes-gcm-siv` being the marginal addition.

### Alternative — Single algorithm but XChaCha20-Poly1305

Random 192-bit nonces resolve the nonce-discipline category outright. Rejected as the *sole* algorithm because it is not NIST-aligned (no FIPS-relevant spec; IRTF CFRG draft + libsodium reference only); compliance auditors in regulated contexts sometimes balk at non-NIST primitives in the first review pass. Worth claiming `0x04` for future inclusion (operators with no FIPS requirement and a strong nonce-discipline preference benefit from it) but not v0.1 default.

### Alternative — Format-versioned-bytes only, no per-ciphertext algo tag

Encode algorithm into the format version (`v1` = GCM-SIV, `v2` = ChaCha20-Poly1305, etc.). Rejected because format versions encode *format* changes (header layout, AAD shape), not algorithm choices — collapsing them ties our hands when we need to bump the format independent of the algorithm. The 1-byte separate `algo_id` is the small price for orthogonality.

### Alternative — AAD = just the column name

Simpler AAD value. Rejected because it does not prevent ciphertext-swap attacks within the same column type — a `medical_certificate_number` ciphertext from row A would still authenticate against row B's decryption attempt. Binding `record_kind:record_id:column` closes that gap.

### Alternative — Tag length below 128 bits

GCM family supports 96- or 104-bit tags for some adopters. Rejected — 128 bits is the standard, costs 4 bytes per row, and the storage savings would be theatrical. Not exposed as a tunable.

### Alternative — Per-call nonce counter instead of random

Eliminates the 96-bit collision risk entirely. Rejected — counter state must be persisted per key (or per replica + per key) and must survive crashes without rewind. The operational complexity dwarfs the random-nonce risk at our write volume. GCM-SIV makes the question moot for the default.

## References

- [ADR-001 §D](ADR-001-platform.md) — envelope encryption posture; this ADR refines the AEAD choice within that posture (Section D's other decisions — DEK ownership, KEK source, crypto-shred — are unchanged).
- [ADR-003 §B/§F](ADR-003-db-migrations.md) — forward-only migration discipline + chunked data rewrites; the §F sweep-job pattern is what an algorithm migration uses.
- [ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md) — `KeyProvider::for_record(record_kind, controller)`; this ADR's AAD shape uses the same `record_kind` identifier.
- [ADR-013 §J](ADR-013-auth-keys.md) — cipher-suite agility precedent on the signing surface, applied here to encryption.
- [ADR-016 §C/§E](ADR-016-compliance-baseline.md) — OWASP ASVS L2 and Cyber Essentials Plus alignment; AEAD with authenticated AAD binding is part of that posture.
- [ADR-018](ADR-018-openapi-emission-format.md) — precedent for "use the safe-Rust impl when one exists" supply-chain hygiene.
- `aes-gcm-siv` crate — <https://docs.rs/aes-gcm-siv/>.
- `chacha20poly1305` crate — <https://docs.rs/chacha20poly1305/>.
- `aes-gcm` crate — <https://docs.rs/aes-gcm/>.
- RFC 8452 — AES-GCM-SIV nonce-misuse-resistant AEAD: <https://datatracker.ietf.org/doc/html/rfc8452>.
- RFC 8439 — ChaCha20 and Poly1305 AEAD: <https://datatracker.ietf.org/doc/html/rfc8439>.
- NIST SP 800-38D — AES-GCM specification: <https://csrc.nist.gov/publications/detail/sp/800-38d/final>.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 24 (no pretence — one cipher does not fit all futures), 28 (truth — the nonce-discipline cost of GCM is real, not theatre), 35–36 (restraint — three audited impls are the minimum to give agility; we do not ship more), 48 (watchfulness — defense in depth on the nonce-discipline category).

## Notes

The algorithm-agility property is the durable artefact. Operators can choose; we can migrate; new algorithms can join; deprecated ones can be swept out. The specific shipped set is a v0.1 snapshot — additions go in `0x04+` reserved space without disturbing existing ciphertexts.

If a vulnerability is disclosed against `aes-gcm-siv` specifically (the marginal addition in the shipped set), the response is: configure operators to switch the default to `chacha20-poly1305` immediately (no migration required for forward writes), then run the §F sweep job to re-encrypt existing GCM-SIV ciphertexts. The agility property turns what would be a forced downtime into a chunked background job.

The deferred algorithms (XChaCha20-Poly1305, AEGIS-256, Ascon-128a) all warrant inclusion when concrete operator demand or post-quantum guidance materialises. They are reserved in §A so the algo_id space does not get squatted by ad-hoc allocations.
