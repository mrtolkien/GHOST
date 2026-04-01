---
name: db-migrations
description: >-
  SQLite migration rules for the Ghost codebase. MUST READ before writing or modifying
  any file in migrations/. Covers: table recreation patterns, column ordering, CHECK
  constraint changes, NOT NULL safety, and the validation checklist. A faulty migration
  that ships breaks the production daemon on startup with no recovery path short of
  manual DB surgery — these rules are non-negotiable.
---

# SQLite Migration Rules

Ghost uses sqlx migrations (`migrations/*.sql`). Migrations run automatically on daemon
boot. **A broken migration kills the daemon** — the GHOST instance goes down and stays
down until someone manually intervenes on the database. There is no rollback mechanism.
Every migration must be correct on the first attempt.

## Naming

Files: `NNN_short_description.sql` (zero-padded sequence number). Add a comment at the
top explaining _why_ the migration exists.

## Table Recreation (CHECK / constraint changes)

SQLite does not support `ALTER TABLE ... ALTER CONSTRAINT`. To change a CHECK
constraint, column type, or other table-level constraint, you must recreate the table:

```sql
CREATE TABLE foo_new ( ... );
INSERT INTO foo_new (...columns...) SELECT ...columns... FROM foo;
DROP TABLE foo;
ALTER TABLE foo_new RENAME TO foo;
```

### Column Ordering (NON-NEGOTIABLE)

**NEVER use `SELECT *` when copying data between tables.** `SELECT *` returns columns in
the _source_ table's physical order, which may not match the new table if columns were
added by previous `ALTER TABLE ADD COLUMN` statements (which always append to the end).

Always explicitly list every column in both the INSERT target and the SELECT source:

```sql
-- WRONG: column order mismatch silently corrupts data or violates constraints
INSERT INTO foo_new SELECT * FROM foo;

-- CORRECT: explicit columns, order doesn't matter because names are mapped
INSERT INTO foo_new (id, name, config, created_at, updated_at)
SELECT id, name, config, created_at, updated_at
FROM foo;
```

This is the rule that prevents the class of bug where a nullable column's NULL value
lands in a NOT NULL column due to position mismatch, crashing the daemon on startup.

### Default Values for New NOT NULL Columns

When adding a NOT NULL column that didn't exist in the old table, provide a default in
the SELECT:

```sql
INSERT INTO foo_new (id, name, new_status, created_at)
SELECT id, name, 'active', created_at
FROM foo;
```

## ALTER TABLE ADD COLUMN

Safe for adding nullable columns or columns with defaults. Remember that `ADD COLUMN`
always appends to the end of the physical column order — this is why explicit column
lists matter in future table recreations.

## Connection Locking

Migrations run on a **dedicated single connection** before the pool is opened
(`src/db/connection.rs`). This is intentional — SQLite DDL (`CREATE TABLE`,
`DROP TABLE`) requires an exclusive lock. If migrations ran through the pool, other pool
connections holding read locks would cause `SQLITE_LOCKED` (code 6) errors on any DDL
migration.

**Do not change this.** If you see migrations running through a pool, that's a bug.

## Validation Checklist

Before committing any migration:

1. **Column list**: Every `INSERT INTO ... SELECT` uses explicit column names on both
   sides. No `SELECT *`.
2. **NOT NULL safety**: Every NOT NULL column in the target has a matching non-null
   source column or an explicit default value.
3. **Constraint coverage**: CHECK constraints, UNIQUE constraints, and foreign keys in
   the new table are compatible with existing data.
4. **Index recreation**: If you DROP a table, its indexes are dropped too. Recreate any
   needed indexes after the RENAME.
5. **FTS/trigger rebuild**: If the table has FTS triggers, they reference the table by
   name. After RENAME, verify triggers still point to the right table.
6. **Test locally**: Run the migration against a copy of the production database before
   shipping. `cp ghost.db ghost_test.db` and run against the copy.
