# ADR-003 — Database migration discipline

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-28 |
| **Deciders** | @ICreateThunder |
| **Tags** | database, migrations, rollback, postgres |
| **Supersedes** | (none) |

## Context

Flight Academy stores regulated aviation data: training progress, pilot ratings, medical certificate metadata, maintenance records, safety occurrences, and the financial records that attach to all of it. Some of this data has statutory retention periods (CAP 382 §6 mandates five years for occurrence data; EASA Part-FCL and Part-145 impose their own); some of it is the subject of GDPR data-subject rights. None of it is data we can afford to lose to a careless schema change.

[ADR-002](ADR-002-release-deployment.md) established two operational facts that bind every decision here:

- **Rolling deployments.** During a canary (ADR-002 §E), pods running code version N and code version N-1 serve traffic against the *same* database at the *same* time. A schema change that the old code cannot tolerate breaks production the moment the migration lands, before the canary has shifted a single percent of traffic.
- **Schema rollback is forbidden.** ADR-002 §J records the asymmetry: image rollback is a routine `git revert`; database schema rollback is not done, because a reverse migration that drops a column or table also drops every row written since the forward migration ran. There is no general way to reconcile that lost data back. The cure is worse than the disease.

That second fact forces a particular discipline. If we cannot roll a schema back, then every forward migration must be safe to deploy *and* safe to leave in place while the code that depends on it is rolled back. The schema must always be one step ahead of, or compatible with, both the current and the previous code release.

The constraints that shaped these decisions:

- **Regulatory.** Data loss in a regulated record store is not merely an availability incident; it can be a compliance breach. Migrations must be designed so that the safety net of last resort — point-in-time recovery — is never the *primary* rollback mechanism.
- **Operational.** Solo maintainer; small ARM nodes; no full-time SRE on call to babysit a migration. Migrations must run unattended, atomically, and either succeed cleanly or fail cleanly with logs that point at the cause.
- **Cross-system consistency.** Several operations span the database *and* an external system — sending a booking confirmation email, charging a card and recording the invoice, syncing an invoice to Xero and marking it synced. These cannot share a database transaction with the external call. We need a pattern that does not leave the two systems disagreeing about what happened.
- **Ethical.** [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instrument 38 ("Be not lazy" — the migration you should have run at the maintenance window you let slip) and instrument 28 ("Utter only truth from heart and mouth" — write what you would want to find at 02:00) both bear directly on migration practice: do the boring discipline now, and leave behind a schema history that the next person can trust.

What we do not yet know at the time of writing: the eventual size of the largest tables (which determines how aggressive the chunking in §F needs to be); whether self-hosters will run migrations through the same Job mechanism or invoke the binary directly; whether we will ever need a read-model / CQRS split that would change the outbox design in §D. These are tracked as open questions and may motivate later ADRs.

This ADR records *how schema changes are made and deployed safely*. The deployment-time mechanics (the Job, the Flagger webhook, the role separation) were introduced in ADR-002 §F; this document is the full discipline they were pointing at.

## Decision

We adopt a six-part migration discipline, labelled A–F. They reinforce one another: forward-only transactional migrations (A) are only safe to leave un-reversed because every change is backward-compatible (B); backward compatibility is only meaningful because the migration runs as a discrete step before traffic shifts (C); cross-system writes that cannot be transactional are made consistent through the outbox (D); and the whole lot is kept honest by schema validation in CI (E) and the safety practices in (F). Each subsection states the choice in one sentence and then expands.

### A. Forward-only, transactional migrations

**Decision: schema changes are sqlx-cli migrations, each running inside a single transaction by default, applied forward-only; reverse ("down") migrations are not generated and not run in normal operation.**

Migrations live in `crates/fa-db/migrations/`, named with a timestamp prefix and a description, e.g. `20260528120000_add_pilots_ratings.sql`. sqlx-cli orders them lexicographically by the prefix and records each applied migration — version, description, checksum, execution time, success — in the `_sqlx_migrations` table. The checksum means a migration file that is edited after it has been applied is detected as drift and the migration run fails rather than silently diverging. Migration files are immutable once merged; a mistake is corrected by a *new* migration, never by editing a landed one.

Each migration runs in a transaction. This is the property that makes forward-only safe: a migration either applies completely or not at all. There is no partial-application state to clean up, no "we got halfway through the `ALTER TABLE` and the connection dropped" recovery problem. PostgreSQL's transactional DDL is the feature that lets us make this guarantee — `CREATE TABLE`, `ALTER TABLE`, `CREATE INDEX` (non-concurrent), `ADD CONSTRAINT`, and most other DDL participate in the surrounding transaction and roll back atomically on error.

A typical migration:

```sql
-- 20260528120000_add_pilots_ratings.sql
-- Adds the new ratings column. Nullable: existing rows are unaffected and
-- code at the previous release continues to work (it does not read this column).
ALTER TABLE pilots
    ADD COLUMN ratings jsonb;
```

Forward-only is the rule because reverse migrations are a data-loss hazard, not a safety feature (see ADR-002 §J and the *Alternatives considered* below). Instead of reversing a bad change, we roll the *code* back (cheap, safe — ADR-002 §J) and design every schema change so that rolling the code back does not require rolling the schema back. That design discipline is §B.

#### The non-transactional exception

A small number of operations cannot run inside a transaction. The most common is `CREATE INDEX CONCURRENTLY`, which PostgreSQL forbids inside a transaction block because it builds the index without taking the long write lock that the transactional form requires — exactly what we want on a large, live table, but at the cost of transactionality. `DROP INDEX CONCURRENTLY`, `ALTER TYPE ... ADD VALUE` (enum extension, in some versions), and `VACUUM` are in the same category.

For these, sqlx supports a per-migration directive that disables the wrapping transaction:

```sql
-- 20260601090000_index_bookings_aircraft_id.sql
-- no-transaction: CREATE INDEX CONCURRENTLY cannot run inside a transaction.
-- RECOVERY: if this fails, an INVALID index may remain. Drop it by hand
--   (DROP INDEX CONCURRENTLY IF EXISTS idx_bookings_aircraft_id;) and re-run.
-- sqlx directive:
-- migrate:no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_bookings_aircraft_id
    ON bookings (aircraft_id);
```

These carry extra responsibility because the atomicity guarantee no longer holds: a failure partway through can leave the database in an intermediate state (a `CONCURRENTLY`-built index that failed leaves an `INVALID` index behind). Every non-transactional migration **must** carry a header comment documenting the partial-failure state and the manual recovery procedure, and must be idempotent on re-run (`IF NOT EXISTS` / `IF EXISTS`). We use these sparingly and only where the alternative — a long-held lock on a busy table — would itself be an outage.

### B. Backward-compatible-always (the parallel-change / expand–contract pattern)

**Decision: every migration must leave the schema usable by both the current code release and the immediately previous one; breaking changes are split into expand and contract steps across separate releases, following the parallel-change (expand-and-contract) refactoring pattern.**

The core invariant, restated precisely:

> Code version N and code version N-1 must both function correctly against schema version N.

This is forced by the rolling deploy. During a canary, old and new pods coexist; the migration has already run (§C) so the schema is at N while some pods are still at code N-1. If schema N is not backward-compatible with code N-1, those pods break. It is *also* forced by rollback: if the canary fails and we revert the code to N-1 (ADR-002 §J), schema N stays in place — it is not reversed — so N-1 must keep working against it.

The general shapes:

| Change | Safe in one step? | Approach |
| --- | --- | --- |
| Add a column | Yes, if nullable or has a default | Single expand migration; old code ignores it |
| Add a table | Yes | Single expand migration; old code does not reference it |
| Add an index (large table) | Yes, with `CONCURRENTLY` | Non-transactional migration (§A exception) |
| Remove a column | No | Two steps: (1) stop reading it in code and deploy; (2) drop it in a later release |
| Rename a column | No | Three steps: add new, dual-write in code, deprecate and drop old later |
| Change a column's type | No | Add new column, migrate data, switch reads, drop old |
| Add a `NOT NULL` constraint | No | Add column with default first, backfill, then `SET NOT NULL` |
| Add a unique constraint | No | Build unique index `CONCURRENTLY`, then attach constraint using it |

The rule of thumb: **a single migration may only make the schema more permissive than the code that is about to be deployed requires.** Anything that removes a permission the old code relies on — a column it reads, a nullability it assumes — must wait until no deployed code relies on it.

#### Worked example — renaming `pilots.class_ratings` to `pilots.ratings`

This is the canonical case because a rename looks atomic at the SQL level (`ALTER TABLE ... RENAME COLUMN`) but is a breaking change: the instant the column is renamed, every pod running the old code that still selects `class_ratings` gets an error. We spread the rename across three releases.

Assume the starting point is schema v0.1.x with a column `pilots.class_ratings jsonb NOT NULL` and code that reads and writes it.

**Release v0.2.0 — expand and dual-write.**

Migration:

```sql
-- 20260602100000_add_pilots_ratings.sql
-- Expand step for the class_ratings -> ratings rename.
-- New column is nullable so existing rows and old code are unaffected.
ALTER TABLE pilots
    ADD COLUMN ratings jsonb;
```

Application code at v0.2.0:

- **Reads** still come from the old column `class_ratings` (the source of truth this release).
- **Writes** go to *both* `class_ratings` and `ratings` (dual-write), keeping them in lockstep for every row touched after this release ships.
- The new column is left nullable; rows not yet written remain `NULL` in `ratings`.

This release is fully backward-compatible: code v0.1.x ignores the new nullable column entirely, and code v0.2.0 still treats `class_ratings` as authoritative. If v0.2.0's canary fails, reverting to v0.1.x is safe — the extra nullable column harms nothing.

**Release v0.3.0 — backfill and switch reads.**

Backfill migration (this is a data update, so it follows the chunking discipline of §F for a large table; shown here as a single statement for clarity):

```sql
-- 20260616100000_backfill_pilots_ratings.sql
-- Copy historical values into the new column for rows written before v0.2.0
-- (i.e. rows the dual-write never touched). Idempotent: only fills NULLs.
UPDATE pilots
    SET ratings = class_ratings
    WHERE ratings IS NULL;
```

Application code at v0.3.0:

- **Reads** now come from the new column `ratings`.
- **Writes** still go to *both* columns (dual-write continues), so that if v0.3.0 is rolled back to v0.2.0 — which reads `class_ratings` — that column is still current.

After this release, `ratings` is the source of truth for reads, but `class_ratings` is still being kept up to date, which is what makes the rollback to v0.2.0 safe. We do *not* drop `class_ratings` yet, and we do *not* yet add `NOT NULL` to `ratings` (a NOT NULL added here would break v0.2.0, which can still write a row without setting `ratings`… it cannot, because v0.2.0 dual-writes — but we keep the rule simple and defer the constraint).

**Release v0.4.0 — contract and drop.**

By the time v0.4.0 ships, no deployed code reads `class_ratings`, and we have decided the rollback horizon to v0.2.0 has passed. Now the old column can go, and the new column can be tightened:

```sql
-- 20260630100000_drop_pilots_class_ratings.sql
-- Contract step. No deployed code references class_ratings.
-- DESTRUCTIVE: drops the legacy column. Reason: completes the
--   class_ratings -> ratings rename begun in v0.2.0; new column is
--   authoritative and fully backfilled. Reviewed-by: @ICreateThunder.
ALTER TABLE pilots
    ALTER COLUMN ratings SET NOT NULL;

ALTER TABLE pilots
    DROP COLUMN class_ratings;
```

Application code at v0.4.0:

- **Reads and writes** use only `ratings`. Dual-write is removed.

Rolling v0.4.0 back to v0.3.0 is safe: v0.3.0 reads `ratings` (present) and writes both `ratings` and `class_ratings` — but `class_ratings` is now gone, so the dual-write to it would fail. This is the one rollback edge: **once the contract migration lands, rollback past it is no longer safe.** That is acceptable and is precisely why the contract step waits until the rollback horizon has passed. The expand and switch steps (v0.2.0, v0.3.0) are freely reversible at the code level; the contract step (v0.4.0) is the point of no return, taken deliberately.

#### Coexistence timeline

The table below shows which schema and code versions coexist and why each combination works. "DW" = dual-write.

| Time | Schema | Code in flight | `class_ratings` | `ratings` | Why it works |
| --- | --- | --- | --- | --- | --- |
| T0 | v0.1.x | v0.1.x | source of truth | absent | Baseline |
| T1 | v0.2.0 (col added) | v0.1.x and v0.2.0 | read source; v0.2.0 also writes (DW) | nullable; v0.2.0 writes (DW) | Old code ignores new nullable column |
| T2 | v0.2.0 | v0.2.0 | read source; DW | populated for new rows; DW | Single code version; `class_ratings` authoritative |
| T3 | v0.3.0 (backfilled) | v0.2.0 and v0.3.0 | DW keeps it current | read source; fully backfilled | v0.2.0 reads `class_ratings` (current via DW); v0.3.0 reads `ratings` |
| T4 | v0.4.0 (col dropped, NOT NULL) | v0.3.0 and v0.4.0 (transient) | dropped | sole source of truth, NOT NULL | v0.3.0 still reads `ratings`; its DW to `class_ratings` is the known edge — contract taken only after rollback horizon passed |

The discipline is more verbose than a one-line `RENAME COLUMN`, and that verbosity is the point: it is what lets a bad release roll back without taking data with it.

### C. Migration execution — a separate Kubernetes Job, not application startup

**Decision: migrations run in a dedicated Kubernetes Job triggered by Flagger's pre-rollout webhook before any traffic shifts; they never run from the application pod on startup; the Job uses a DDL-capable Postgres role that the application pods do not have.**

This was introduced in ADR-002 §F; the rationale in full:

- **Avoiding a race across replicas.** If migrations ran on application startup, every pod in a multi-replica Deployment would try to migrate at once, requiring an application-level advisory lock and turning a schema change into a distributed-coordination problem. A single Job runs once, to completion, with clear logs.
- **Ordering relative to the canary.** Flagger's `pre-rollout` webhook (ADR-002 §E) fires the Job *before* the canary begins shifting traffic. Only on Job success does traffic move onto the new pods. A failed migration fails the rollout cleanly — no traffic ever reaches code that expected a schema that was not applied.
- **Blast-radius containment via role separation.** The Job authenticates as `app_migrator`, which holds DDL privileges. The running API pods authenticate as `app_api`, which has no DDL at all. A compromised API pod therefore cannot alter the schema, drop a table, or disable a constraint — it is confined to the DML that row-level security (RLS) already scopes to its tenant context. The DDL capability exists only inside a short-lived Job with its own service account and its own credentials, never in the long-lived pods exposed to the internet.

Database roles:

| Role | Privileges | Used by |
| --- | --- | --- |
| `app_migrator` | DDL on the application schema; DML during migrations and backfills | The migration Job only |
| `app_api` | DML within RLS policies; **no DDL** | The running API pods and workers |
| `app_backup` | `SELECT` only (read-only) | The backup CronJob |

An illustrative migration Job:

```yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: flight-academy-migrate-v0-4-0
  namespace: flight-academy
spec:
  backoffLimit: 0            # no retries: a failed migration is investigated, not blindly re-run
  activeDeadlineSeconds: 600
  template:
    spec:
      restartPolicy: Never
      serviceAccountName: app-migrator   # bound to the app_migrator DB credential source
      containers:
        - name: migrate
          image: ghcr.io/flight-academy-uk/flight-academy:v0.4.0
          command: ["/usr/local/bin/flight-academy"]
          args: ["migrate", "run"]       # sqlx migrate run against crates/fa-db/migrations
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: app-migrator-dsn   # injected for the Job's SA only
                  key: dsn
```

`backoffLimit: 0` and `restartPolicy: Never` are deliberate: a migration that fails should surface as a single failed Job with readable logs, and a human should look at *why* before anything re-runs. A transactional migration that failed rolled itself back (§A), so re-running after a fix is safe; a non-transactional one carries its recovery note (§A exception). Flagger treats Job failure as a failed pre-rollout hook and does not shift traffic.

For self-hosters, the same `migrate run` subcommand is invoked by the install script's idempotent upgrade path (ADR-002 §I) rather than by a Kubernetes Job; the binary and the migration set are identical.

### D. Cross-system transactionality — the transactional outbox

**Decision: operations that span the database and an external system use the transactional outbox pattern — the state change and an `events_outbox` row are committed in one database transaction, and a worker tails the outbox and dispatches to idempotent handlers. We do not use distributed transactions, and we do not publish to an external broker before the database commit.**

#### The problem

Several operations must change local state *and* cause an effect in an external system:

- A booking is confirmed *and* a confirmation email is sent.
- A card is charged *and* an invoice row is created.
- An invoice is synced to Xero *and* marked synced locally.

There is no shared transaction across PostgreSQL and Stripe, or PostgreSQL and an SMTP server, or PostgreSQL and Xero. If we write the database row and then call the external system, a crash in between leaves the external effect un-triggered. If we call the external system first and then write the row, a crash leaves an external effect (a charged card, a sent email) with no local record — and if the database write then *rolls back*, we have "published an event for a write that never happened." This last failure is the one that broker-first designs relitigate endlessly.

#### The pattern

The state change and a row describing the event are written in the **same** database transaction:

```sql
-- Within one application transaction:
INSERT INTO bookings (id, tenant_id, aircraft_id, slot, status, ...)
    VALUES ($1, $2, $3, $4, 'confirmed', ...);

INSERT INTO events_outbox (id, tenant_id, kind, payload, occurred_at, dispatched_at)
    VALUES ($5, $2, 'booking.confirmed', $6, now(), NULL);
-- COMMIT
```

Because both inserts are in one transaction, either both land or neither does. There is no window in which the booking exists without its event, or the event exists without its booking. The `events_outbox` row is the durable, atomic record that "this thing happened and an effect is owed."

A worker then dispatches:

- It tails the outbox using `LISTEN`/`NOTIFY` for low-latency wake-ups, with a periodic polling sweep as a fallback so that a missed notification (e.g. the worker was restarting) is still picked up. Polling-only would add latency; `LISTEN`/`NOTIFY`-only would risk a lost wake-up. Both together are belt-and-braces.
- For each undispatched row it invokes the registered handler for that `kind` (send the email, call Stripe, push to Xero).
- On success it sets `dispatched_at`. On failure it leaves the row, backs off, and retries.

Properties that fall out of this design:

- **Idempotent handlers, keyed by event ID.** Because a row may be dispatched more than once (the worker crashed after the external call but before setting `dispatched_at`), every handler must be safe to run twice for the same event ID. This is a requirement on handlers, enforced by convention and tested.
- **Replayable.** Resetting a worker's offset (or clearing `dispatched_at` for a range) replays events. This is how we recover from a handler bug: fix the handler, replay.
- **Auditable.** Every event is a queryable row with a tenant scope and a timestamp. Combined with CNPG point-in-time recovery, the event history is reconstructable to any point in time. This dovetails with the audit posture in ADR-001 and the events-for-bookings rationale there.

#### External-call idempotency — the persisted state machine

For handlers whose external call is itself not safely repeatable — charging a card is the canonical example, where a naive retry could double-charge — the handler uses an **idempotency key** plus a persisted state machine with explicit phase markers:

```text
intent written  ->  external call made  ->  done written
   (DB row)            (Stripe charge,        (DB row updated
                        idempotency-keyed)     with result)
```

The phases:

1. Before calling the external system, persist an `intent` row carrying a stable idempotency key (derived from the event ID).
2. Make the external call, passing that idempotency key so the provider de-duplicates on their side if we retry.
3. On success, persist `done` with the provider's result.

On restart, the handler inspects the phase: if `intent` is present but `done` is not, it resumes by re-issuing the external call *with the same idempotency key* — the provider returns the original result rather than charging again. The local state machine plus the provider's idempotency key together close the double-effect window that the outbox alone does not.

This is the same idempotency discipline ADR-001 §E describes for webhook *receipt* (`webhook_events` de-duplication); here it is applied to webhook/API *emission*.

### E. Schema validation in CI

**Decision: CI applies all migrations to a fresh ephemeral Postgres, dumps the resulting schema, and fails the build if it differs from a checked-in canonical `crates/fa-db/schema.sql`; CI additionally verifies that the compile-time-checked queries match the live schema.**

The job, to be added to the `ci` workflow when the database layer lands:

```yaml
# Sketch of the schema-validation job (to be added to .github/workflows/ci.yml).
schema-check:
  runs-on: ubuntu-latest
  services:
    postgres:
      image: postgres:16
      env:
        POSTGRES_PASSWORD: ci
      ports: ["5432:5432"]
  steps:
    - uses: actions/checkout@v4
    - name: Apply all migrations to a fresh database
      run: sqlx migrate run --source crates/fa-db/migrations
    - name: Dump the resulting schema
      run: pg_dump --schema-only --no-owner --no-privileges "$DATABASE_URL" > /tmp/schema.actual.sql
    - name: Diff against the committed canonical schema
      run: diff -u crates/fa-db/schema.sql /tmp/schema.actual.sql
    - name: Verify compile-time-checked queries match the live schema
      run: cargo sqlx prepare --check --workspace
```

What this catches:

- **Accidental manual schema changes.** A column added by hand in a developer's local database that never made it into a migration shows up as drift, because the migrations applied to a clean database would not reproduce it.
- **Migrations that do not produce the expected schema.** A migration that was edited to do one thing but actually does another is caught when the dump does not match the committed `schema.sql`.
- **Divergence between "what a developer applied locally" and "what the migrations actually do."** The canonical `schema.sql` is the single agreed picture of the schema; the migration sequence must reproduce it exactly.
- **Query/schema skew.** sqlx checks queries against the database at compile time; `cargo sqlx prepare --check` against a freshly-migrated database verifies that the committed query cache (`.sqlx/`) still matches the schema the migrations produce. A query referencing a column a migration removed fails here, not in production.

`crates/fa-db/schema.sql` is committed and reviewed like any other source file. Updating it is part of the same PR that adds a migration; a migration PR that forgets to regenerate the canonical schema fails the diff and cannot merge. The CI placeholder in `.github/workflows/ci.yml` already anticipates the Rust matrix; this ADR mandates that the `schema-check` job above be added as part of it when the DB layer lands.

### F. Migration safety practices

**Decision: large data changes are chunked and throttled; migrations are tested against production-shaped data in CI before reaching production; destructive operations require documented justification and review; and point-in-time recovery is the safety net of last resort, never the primary rollback path.**

- **Chunked, throttled data updates.** A migration that updates millions of rows in a single statement holds locks for the duration, bloats the WAL, and lags the synchronous replicas (ADR-002 §G describes the three-replica synchronous CNPG cluster). Large backfills are written to update in bounded batches (e.g. by primary-key range or `LIMIT`-ed `UPDATE ... WHERE id IN (...)` loops), committing each batch and pausing briefly between them so replication and autovacuum keep up. The v0.3.0 backfill in §B is shown as one statement for clarity; on a large `pilots` table it would be chunked.
- **Tested against production-shaped data.** Beyond the clean-schema diff in §E, migrations that transform data are exercised in CI against a dataset shaped like production (anonymised or synthetic, never real regulated data) so that timing, lock behaviour, and correctness are observed before the migration reaches a real cluster.
- **Destructive operations gated.** Any `DROP`, `TRUNCATE`, or other irreversible operation must carry a header comment in the migration file stating *what* is being destroyed, *why* it is safe (which release stopped depending on it), and a reviewer attribution — as shown in the v0.4.0 contract migration in §B. A destructive migration without that justification is rejected in review.
- **PITR is the last resort, not the plan.** CNPG continuous WAL archiving to object-locked S3 (ADR-002 §G) gives point-in-time recovery. PITR is genuine insurance for the catastrophic case — a migration that corrupted data in a way the design failed to anticipate. But the entire discipline above (transactional migrations, backward compatibility, the dedicated Job, CI validation) exists so that PITR is *never the first answer*. Recovering by rolling the whole database back in time loses every write since the recovery point for every tenant — it is a tenant-wide outage and data-loss event, acceptable only when the alternative is worse. Migrations are designed so we never reach for it.

## Consequences

### Positive

- **Rollback stays safe.** Because every schema change is backward-compatible (§B), the routine `git revert` image rollback from ADR-002 §J works even across schema changes. The two ADRs together make "roll the code back, leave the schema" a sound operational rule.
- **No partial-migration states in the common case.** Transactional migrations (§A) either fully apply or fully roll back. The non-transactional exceptions are explicitly marked and carry recovery notes.
- **Schema cannot silently drift.** The CI diff against `schema.sql` plus the `_sqlx_migrations` checksums (§A, §E) mean a hand-edited database, an edited landed migration, or a query referencing a dropped column all fail the build rather than surfacing in production.
- **DDL blast radius is contained.** Role separation (§C) means a compromised API pod cannot change the schema. DDL lives only in a short-lived Job.
- **Cross-system writes are consistent.** The outbox (§D) eliminates the "external effect with no local record" and "local record with no external effect" failure modes without distributed transactions, and the event log is auditable and replayable.
- **Data loss requires deliberate action.** The only point at which rollback becomes unsafe is the contract step (§B), which is taken consciously after a rollback horizon. There is no accidental path to data loss through a routine deploy.

### Negative

- **Every breaking change is a multi-release dance.** A rename is three releases; a type change is similar. This is more developer time per change than a single `ALTER TABLE`, and it requires planning ahead. This cost is accepted deliberately — it is the price of safe rollback (ADR-002 §J already names it).
- **Dual-write code is carried temporarily.** Between the expand and contract steps, application code writes both old and new shapes. This is extra code that must be remembered and removed at the contract step; forgetting to remove it is a (benign) source of cruft.
- **The outbox adds a worker and a table.** The transactional outbox is more moving parts than a direct call: a worker process, a polling sweep, idempotent handlers, a state machine for non-repeatable calls. For a solo maintainer this is real operational surface, justified by the consistency it buys.
- **Non-transactional migrations carry manual recovery risk.** The `CONCURRENTLY` exception (§A) trades atomicity for lock-friendliness; a failure there can require hand recovery. Mitigated by the mandatory recovery-note convention and idempotent `IF [NOT] EXISTS` guards, but the risk is real.
- **CI must run a real Postgres.** The schema-check and data-shape tests (§E, §F) require an ephemeral Postgres in CI, adding minutes and a service container to the run. Accepted as the cost of catching drift before production.

### Neutral

- **Forward-only means the migrations directory only grows.** Old migrations are never removed; the history is the record. `schema.sql` is the readable snapshot for anyone who does not want to read the whole sequence. A future squash of very old migrations into a baseline is possible but not planned.
- **The outbox is not a general event bus.** It is a reliability mechanism for cross-system writes, not a public eventing API. If we later need genuine inter-service eventing, the outbox can feed a broker (outbox-then-broker), but that is a future decision (see ADR-004, forthcoming, and the open questions in Context).
- **`sqlx` couples us to compile-time query checking.** The `cargo sqlx prepare --check` step (§E) ties CI to the committed query cache. This is a deliberate trade — compile-time query verification is a strong correctness property — but it means the query cache is a reviewed artefact like `schema.sql`.

## Alternatives considered

### Alternative — reversible "down" migrations

The conventional migration-framework approach: every `up` has a matching `down`, and rollback runs the `down`. Rejected because, for a regulated data store, a `down` that drops a column or table also drops every row written since the `up` ran, with no general way to recover it (ADR-002 §J states this as a settled principle). "Down" migrations give the *feeling* of reversibility while quietly being a data-loss mechanism. We get real reversibility where it matters — at the code level — through backward-compatible forward-only migrations instead. We would reconsider only for a class of schema change that is provably lossless to reverse, and even then the operational simplicity of "we never run down migrations" is worth more than the occasional convenience.

### Alternative — running migrations from application startup

Each pod runs pending migrations as it boots. Simpler to wire up; no separate Job. Rejected (also in ADR-002 §F's alternatives) because every replica races to migrate, forcing an application-level lock; because the API pod's role would then need DDL privileges it must not have (§C); and because a migration failure would crash-loop the application instead of surfacing as one clear Job failure before any traffic shifts. The dedicated Job is what lets `app_api` stay strictly DML-only.

### Alternative — ORM auto-migration (auto-DDL)

Some frameworks derive the schema from model definitions and auto-apply `ALTER`s to reconcile the database to the code ("auto-migrate"). Rejected outright. Auto-DDL hides the exact change behind framework heuristics, frequently generates a destructive operation (drop-and-recreate, column retype) without warning, gives no place to encode the expand/contract discipline (§B), and produces no reviewable, checksummed migration artefact. For regulated data this is unacceptable: we require that every schema change be an explicit, reviewed, immutable SQL file. We use sqlx for *query* checking, not for schema generation.

### Alternative — a message broker (Kafka / NATS) for cross-system events

Publish events to a broker and let consumers act. Rejected as the consistency mechanism (it remains available as a future *transport*, see Neutral) because publishing to a broker is a separate system from the database commit: you either publish-then-commit (and risk an event for a write that rolled back) or commit-then-publish (and risk a committed write whose event was never published if the process dies in between). This is the dual-write problem the outbox exists to solve. A broker also adds a stateful, memory-hungry component to a cost-constrained ARM cluster (ADR-002 Context) for which there is no current need. The transactional outbox gives at-least-once delivery with database-strong consistency and no extra infrastructure. We would add a broker only when genuine multi-consumer, high-throughput eventing is required, and even then it would sit *behind* the outbox.

### Alternative — two-phase commit / distributed transactions (XA)

Coordinate PostgreSQL and the external system in one distributed transaction. Rejected because the external systems (Stripe, SMTP, Xero) do not offer an XA-compatible transaction manager, because 2PC has well-known availability and coordinator-failure pathologies, and because it would couple our commit latency to third-party availability. The outbox achieves practical consistency without requiring participants to support distributed transactions they do not, in fact, support.

### Alternative — big-bang migration with a maintenance window

Take the service down, run the breaking migration in one step, bring it back up on the new code. Simpler per change — no expand/contract, no dual-write. Rejected because it requires planned downtime (incompatible with the zero-downtime canary in ADR-002 §E), because it makes rollback an all-or-nothing event (the breaking change is already applied), and because it scales badly with tenant count and table size — the window grows with the data. Maintenance windows are a tool we keep for genuinely exceptional operations, not the default for routine schema evolution. The expand/contract discipline (§B) is more work per change but needs no window and keeps rollback safe.

## References

### Related ADRs

- [ADR-001 — Platform architecture](ADR-001-platform.md) — §E (integrations, webhook idempotency) and the events-for-bookings and audit rationale inform the outbox design in §D; the envelope-encryption model in §D constrains what migrations may touch.
- [ADR-002 — Release and deployment](ADR-002-release-deployment.md) — §E (Flagger canary, pre-rollout webhook), §F (migration Job, role separation), §G (CNPG topology, WAL archiving) and §J (rollback discipline, parallel-change rule) are the deployment-time counterpart to this document.
- [ADR-004 — Defence in depth](ADR-004-defence-in-depth.md) — audit-log retention and any future eventing surface (forthcoming).

### Project documents

- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instrument 38 (diligence — the migration not run at the window let slip), instrument 28 (truthful schema history), instrument 48 (watchfulness over destructive operations).
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — `db` scope for conventional commits; tests-around-multi-tenancy expectation applies to migrations.
- [GOVERNANCE.md](../../GOVERNANCE.md) — migration-discipline changes are an ADR-class decision.

### External standards and documentation

- sqlx — <https://github.com/launchbadge/sqlx> (migrations, `_sqlx_migrations` table, compile-time query checking, `cargo sqlx prepare`)
- PostgreSQL — transactional DDL: <https://www.postgresql.org/docs/current/sql-altertable.html>
- PostgreSQL — `CREATE INDEX CONCURRENTLY` (cannot run in a transaction): <https://www.postgresql.org/docs/current/sql-createindex.html#SQL-CREATEINDEX-CONCURRENTLY>
- Parallel Change (expand and contract) — Danilo Sato, refactoring.com / Martin Fowler: <https://martinfowler.com/bliki/ParallelChange.html>
- Transactional Outbox pattern — Chris Richardson, microservices.io: <https://microservices.io/patterns/data/transactional-outbox.html>
- CloudNativePG — point-in-time recovery: <https://cloudnative-pg.io/docs/current/recovery/>

## Notes

The asymmetry that drives this entire ADR — image rollback is cheap, schema rollback is forbidden — was already settled in ADR-002 §J. This document is the working-out of what that asymmetry demands of day-to-day schema practice: forward-only transactional migrations, the expand/contract dance, the dedicated Job, the outbox, and the CI checks that keep it all honest.

The worked example in §B uses `pilots.class_ratings → pilots.ratings` because a rename is the clearest case where an apparently-atomic SQL operation is in fact a breaking change. The same three-step shape — expand, switch, contract — applies to type changes, table splits, and the introduction of `NOT NULL`; the example generalises.

`crates/fa-db/schema.sql` and the `schema-check` CI job described in §E do not yet exist; the CI workflow is a placeholder (`.github/workflows/ci.yml`) until the first feature commits land. This ADR mandates that both be added when the database layer does. Until then, the discipline is recorded so that the first migration written is written correctly, in the spirit of instrument 38 — do the boring discipline now, while there is still time.
