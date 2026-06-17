//! Enforces the replicated-resource invariants from ADR-007 §B/§C/§E and
//! domain-model §7.1. The schema-drift CI gate (ADR-003 §E) catches
//! divergence from the committed `schema.sql`; this test catches the
//! sibling failure mode — a newly-introduced table that opts into the
//! `updated_at` sync feed without the rest of the contract (soft-delete
//! columns, deletion-consistency constraint, BEFORE UPDATE trigger,
//! watermark composite index).
//!
//! Heuristic: any application table that carries `updated_at` is treated
//! as a replicated resource. Tables explicitly excluded:
//!
//! - `_sqlx_migrations` — migration framework's own bookkeeping
//! - `audit_events` and its monthly partitions — INSERT-only per ADR-009
//!   §A; immutability triggers replace soft-delete semantics
//!
//! When the first non-`tenants` replicated table lands (E2 memberships,
//! F1 safety occurrences, G1 aircraft …), this test is the single place
//! that catches a missing `deleted_at`, a missing trigger, or a missing
//! watermark index — before the schema reaches production where ADR-003
//! §B's forward-only discipline makes retrofit expensive.

use flight_academy_test_support::fresh_db;
use sqlx::Row;

fn is_excluded(table: &str) -> bool {
    matches!(table, "_sqlx_migrations" | "audit_events") || is_audit_events_partition(table)
}

/// Matches the `audit_events_YYYY_MM` declarative-partition naming
/// imposed by ADR-009 §E. Avoids accidentally excluding a hypothetical
/// `audit_events_dashboards` table that happened to share the prefix.
fn is_audit_events_partition(table: &str) -> bool {
    let Some(suffix) = table.strip_prefix("audit_events_") else {
        return false;
    };
    let (year, month) = match suffix.split_once('_') {
        Some(pair) => pair,
        None => return false,
    };
    year.len() == 4
        && year.bytes().all(|b| b.is_ascii_digit())
        && month.len() == 2
        && month.bytes().all(|b| b.is_ascii_digit())
}

#[tokio::test]
async fn every_replicated_table_satisfies_adr_007() {
    let db = fresh_db().await;
    let pool = db.pool();

    let tables_with_updated_at: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND column_name  = 'updated_at'
          ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .expect("query columns");

    let candidates: Vec<&str> = tables_with_updated_at
        .iter()
        .map(String::as_str)
        .filter(|t| !is_excluded(t))
        .collect();

    assert!(
        !candidates.is_empty(),
        "no replicated-resource tables found — expected at least `tenants`. \
         If `tenants` was renamed or dropped, update this test's expectations."
    );

    let mut failures: Vec<String> = Vec::new();

    for table in candidates {
        if let Err(msg) = check_table(pool, table).await {
            failures.push(format!("- {table}: {msg}"));
        }
    }

    assert!(
        failures.is_empty(),
        "ADR-007 invariant violations detected:\n{}",
        failures.join("\n")
    );
}

async fn check_table(pool: &sqlx::PgPool, table: &str) -> Result<(), String> {
    check_soft_delete_columns(pool, table).await?;
    check_deletion_consistency_constraint(pool, table).await?;
    check_before_update_trigger(pool, table).await?;
    check_watermark_index(pool, table).await?;
    Ok(())
}

async fn check_soft_delete_columns(pool: &sqlx::PgPool, table: &str) -> Result<(), String> {
    let row = sqlx::query(
        "SELECT
            (SELECT data_type FROM information_schema.columns
              WHERE table_schema='public' AND table_name=$1
                AND column_name='deleted_at') AS deleted_at_type,
            (SELECT data_type FROM information_schema.columns
              WHERE table_schema='public' AND table_name=$1
                AND column_name='deletion_reason') AS deletion_reason_type",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("column lookup failed: {e}"))?;

    let deleted_at: Option<String> = row.try_get("deleted_at_type").ok();
    let deletion_reason: Option<String> = row.try_get("deletion_reason_type").ok();

    if deleted_at.as_deref() != Some("timestamp with time zone") {
        return Err(format!(
            "ADR-007 §C requires `deleted_at timestamptz` — found {deleted_at:?}"
        ));
    }
    if deletion_reason.as_deref() != Some("text") {
        return Err(format!(
            "ADR-007 §C requires `deletion_reason text` — found {deletion_reason:?}"
        ));
    }
    Ok(())
}

async fn check_deletion_consistency_constraint(
    pool: &sqlx::PgPool,
    table: &str,
) -> Result<(), String> {
    let constraint_exists: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_constraint c
              JOIN pg_class r ON r.oid = c.conrelid
             WHERE r.relname = $1
               AND c.contype = 'c'
               AND pg_get_constraintdef(c.oid) ILIKE '%deleted_at%deletion_reason%'
         )",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("constraint lookup failed: {e}"))?;

    if !constraint_exists.unwrap_or(false) {
        return Err(
            "missing CHECK constraint binding deleted_at and deletion_reason \
             (ADR-007 §C — both set or neither)"
                .into(),
        );
    }
    Ok(())
}

async fn check_before_update_trigger(pool: &sqlx::PgPool, table: &str) -> Result<(), String> {
    let has_trigger: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
              FROM information_schema.triggers
             WHERE event_object_schema = 'public'
               AND event_object_table  = $1
               AND event_manipulation  = 'UPDATE'
               AND action_timing       = 'BEFORE'
               AND action_orientation  = 'ROW'
         )",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("trigger lookup failed: {e}"))?;

    if !has_trigger.unwrap_or(false) {
        return Err(
            "missing BEFORE UPDATE FOR EACH ROW trigger to bump `updated_at` \
             (ADR-007 §B — trigger-driven so bulk DML and migrations cannot forget)"
                .into(),
        );
    }
    Ok(())
}

async fn check_watermark_index(pool: &sqlx::PgPool, table: &str) -> Result<(), String> {
    // ADR-007 §E: the watermark index serves RLS scope + updated_since +
    // cursor pagination in one seek. Shape is either `(updated_at, id)`
    // (resource that IS the tenant — e.g. `tenants`) or
    // `(<scope>, updated_at, id)` (resource owned by a tenant/user).
    //
    // We match `updated_at, id)` as the trailing suffix of the indexdef
    // column list. The leading `(` is intentionally NOT part of the
    // pattern — anchoring on it would reject `(tenant_id, updated_at, id)`
    // for tenant-scoped resources (the `(` there precedes `tenant_id`,
    // not `updated_at`). Both 2-col and 3-col shapes pass.
    let has_index: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_indexes
             WHERE schemaname = 'public'
               AND tablename  = $1
               AND indexdef ~ 'updated_at, id\\)\\s*(WHERE|$)'
         )",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("index lookup failed: {e}"))?;

    if !has_index.unwrap_or(false) {
        return Err(
            "missing watermark index ending with (updated_at, id) — required \
             by ADR-007 §E to collapse RLS + updated_since + cursor into one seek"
                .into(),
        );
    }
    Ok(())
}

#[test]
fn excluded_pattern_matches_intended_tables() {
    assert!(is_excluded("_sqlx_migrations"));
    assert!(is_excluded("audit_events"));
    assert!(is_excluded("audit_events_2026_06"));
    assert!(is_excluded("audit_events_2026_07"));

    assert!(!is_excluded("tenants"));
    assert!(!is_excluded("memberships"));
    assert!(!is_excluded("audit_events_dashboards")); // not an audit partition
}
