#!/usr/bin/env bash
# Generates prompts/skills/knowledge-navigator/schema.sql from migrations.
# Called by `just generate-schema` and checked by `just ci`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT="$ROOT/prompts/skills/knowledge-navigator/schema.sql"

tmpdb=$(mktemp /tmp/ghost-schema-XXXX.db)
trap 'rm -f "$tmpdb"' EXIT

for f in "$ROOT"/migrations/*.sql; do
    sqlite3 "$tmpdb" < "$f"
done

{
    echo "-- Auto-generated from migrations/ — do not edit manually."
    echo "-- Regenerate with: just generate-schema"
    echo ""
    # Dump only CREATE TABLE and CREATE INDEX (skip FTS virtual tables,
    # FTS internal tables, and triggers)
    sqlite3 "$tmpdb" ".schema" \
        | grep -v '^CREATE VIRTUAL TABLE' \
        | grep -v '^CREATE TRIGGER' \
        | grep -v "^CREATE TABLE IF NOT EXISTS '" \
        | grep -v '^/\*' \
        | grep -v '^\);\s*$' -v 2>/dev/null || true
} > /dev/null

# Better approach: use sqlite3 to extract each table's DDL individually
{
    echo "-- Auto-generated from migrations/ — do not edit manually."
    echo "-- Regenerate with: just generate-schema"
    echo ""
    sqlite3 "$tmpdb" <<'SQL'
SELECT sql || ';'
FROM sqlite_master
WHERE type IN ('table', 'index')
  AND name NOT LIKE '%_fts%'
  AND name NOT LIKE 'sqlite_%'
  AND sql IS NOT NULL
ORDER BY
  CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 END,
  name;
SQL
} > "$OUT"

echo "Generated $OUT"
