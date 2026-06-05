#!/usr/bin/env bash
# Regenerate crates/flight-academy-db/schema.sql from a freshly-migrated
# database. Run after adding or changing a migration; commit the resulting
# schema.sql in the same PR. The ADR-003 §E CI gate diffs this file against
# a fresh-from-migrations dump and fails the build on drift.
#
# schema.sql is a REFERENCE SNAPSHOT for drift detection, not a replay
# artefact. Provision databases with `flight-academy migrate` (which runs
# the embedded sqlx migrations), not `psql -f schema.sql` — the latter
# strips psql's `\restrict` meta-command protection that this script
# filters out for determinism. Production backup/restore uses CNPG WAL
# archive or operator-driven pg_dump|pg_restore, both of which keep the
# `\restrict` nonce intact end-to-end.
#
# Requires the dev Postgres container to be up (docker-compose.dev.yml).
# Uses pg_dump *inside* the container so the client major version matches
# the server, then strips lines that pg_dump emits non-deterministically:
#   - `\restrict <random-nonce>` / `\unrestrict <random-nonce>` (psql
#     meta-command-injection guard; nonce changes every dump)
#   - version-stamp comments (drift with PG point releases)
#
#   ./scripts/regenerate-schema.sh

set -euo pipefail
cd "$(dirname "$0")/.."

CONTAINER="flight-academy-postgres-1"
DB="flight_academy_dev"
USER="postgres"
OUT="crates/flight-academy-db/schema.sql"

if ! docker exec "$CONTAINER" pg_isready -U "$USER" -d "$DB" >/dev/null 2>&1; then
    echo "Postgres not ready in $CONTAINER — run: docker compose -f docker-compose.dev.yml up -d"
    exit 2
fi

docker exec "$CONTAINER" pg_dump \
    --schema-only \
    --no-owner \
    --no-privileges \
    -U "$USER" -d "$DB" \
    | grep -Ev '^\\(restrict|unrestrict) ' \
    | grep -Ev '^-- Dumped (from database|by pg_dump) version' \
    > "$OUT"

echo "wrote $OUT ($(wc -l <"$OUT") lines)"
