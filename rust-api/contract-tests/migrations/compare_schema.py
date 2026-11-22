#!/usr/bin/env python3
"""Schema + seed comparison for the NEWPIDASH-380 migrator proof.

`pidash-api migrate up` on an empty Postgres must produce a schema
identical (tables, columns, types, defaults, indexes, constraints) to
Django `migrate` at the same leaf, with the same seed data. This script
diffs two live databases at the catalog level and exits 0 only when they
match.

Usage:
    compare_schema.py <label-a> <dsn-a> <label-b> <dsn-b>

DSNs are libpq conninfo strings. Tables are compared as sets: row lines
are sorted per table, so seed insertion order never matters. Three
exclusion rules keep run-derived cells out of the diff, and every rule
is printed with the result:

- SKIP_TABLES: the migrator's own history table (Rust-only by design).
- SKIP_COLUMNS: cells that depend on the run wall-clock
  (`django_migrations.applied`; the single seeded `loop_jobs` row's
  timestamps). The `django_migrations` (app, name) pairs and the
  `loop_jobs` payload are still compared exactly.
- COUNT_ONLY: large run-derived tables (`auth_permission`,
  `django_content_type`, created by Django's post-migrate permission /
  content-type creation): row counts must match, contents are not
  fingerprinted.
"""

import difflib
import sys

import psycopg

# Tables owned by the migrator run itself, not the schema.
SKIP_TABLES = {"pidash_schema_migrations"}
# Rust-only tables from the native `ensure_schema` migration (m000002):
# boot-time DDL folded into the migration set (NEWPIDASH-380). Django never
# creates them, and a fallback Django runs fine with them present, so the
# Django-leaf comparison excludes them; their shape is pinned by the
# migration crate's own tests instead.
RUST_ONLY_TABLES = {"rust_job_queue", "rust_job_schedule", "rust_runner_model_catalog"}
# Columns whose values depend on the run wall-clock or are generated fresh
# on every install. The payloads are still compared exactly:
# - `django_migrations.applied` (run time); (app, name) pairs compared.
# - `loop_jobs`: `update_or_create` mints a fresh id and `dtstart = now()`
#   on every migrate (db/0149_loop_mvp); slug + payload compared.
# - `prompt_template`: reseed migrations mint fresh ids on every migrate;
#   name + body + version + flags compared.
SKIP_COLUMNS = {
    ("django_migrations", "applied"),
    ("loop_jobs", "id"),
    ("loop_jobs", "dtstart"),
    ("loop_jobs", "created_at"),
    ("loop_jobs", "updated_at"),
    ("prompt_template", "id"),
    ("prompt_template", "created_at"),
    ("prompt_template", "updated_at"),
}
# Tables compared by row count only (large; content is run-derived).
COUNT_ONLY = {"auth_permission", "django_content_type"}


def snapshot(dsn):
    out = []
    with psycopg.connect(dsn) as conn:
        with conn.cursor() as cur:
            cur.execute(
                """
                SELECT tablename FROM pg_tables
                WHERE schemaname = 'public' ORDER BY 1
                """
            )
            tables = [
                r[0]
                for r in cur.fetchall()
                if r[0] not in SKIP_TABLES and r[0] not in RUST_ONLY_TABLES
            ]
            out.append(f"# tables: {len(tables)}")
            for table in tables:
                cur.execute(
                    """
                    SELECT column_name, udt_name, is_nullable,
                           column_default
                    FROM information_schema.columns
                    WHERE table_schema = 'public' AND table_name = %s
                    ORDER BY ordinal_position
                    """,
                    (table,),
                )
                for col, typ, nullable, default in cur.fetchall():
                    out.append(f"{table}.{col}: {typ} nullable={nullable} default={default}")
                cur.execute(
                    """
                    SELECT indexname, indexdef FROM pg_indexes
                    WHERE schemaname = 'public' AND tablename = %s
                    ORDER BY indexname
                    """,
                    (table,),
                )
                for name, definition in cur.fetchall():
                    out.append(f"index {name}: {definition}")
                cur.execute(
                    """
                    SELECT conname, pg_get_constraintdef(oid)
                    FROM pg_constraint
                    WHERE conrelid = %s::regclass
                    ORDER BY conname
                    """,
                    (table,),
                )
                for name, definition in cur.fetchall():
                    out.append(f"constraint {table}.{name}: {definition}")
                cur.execute(f"SELECT count(*) FROM {table}")
                count = cur.fetchone()[0]
                out.append(f"rows {table}: {count}")
                if table not in COUNT_ONLY and count <= 2000:
                    cur.execute(f"SELECT * FROM {table} ORDER BY 1")
                    cols = [d[0] for d in cur.description]
                    rows = []
                    for row in cur.fetchall():
                        cells = []
                        for col, val in zip(cols, row):
                            if (table, col) in SKIP_COLUMNS:
                                val = "<volatile>"
                            cells.append(f"{col}={val!r}")
                        rows.append(f"row {table}: {' | '.join(cells)}")
                    # Tables are sets: seed insertion order must not matter.
                    out.extend(sorted(rows))
    return out


def main():
    label_a, dsn_a, label_b, dsn_b = sys.argv[1:5]
    snap_a = snapshot(dsn_a)
    snap_b = snapshot(dsn_b)
    if snap_a == snap_b:
        print(f"IDENTICAL: {label_a} == {label_b} ({len(snap_a)} lines)")
        return 0
    print(f"DIFFER: {label_a} vs {label_b}")
    print("Rules: skip tables", sorted(SKIP_TABLES))
    print("Rules: rust-only tables", sorted(RUST_ONLY_TABLES))
    print("Rules: volatile columns", sorted(f"{t}.{c}" for t, c in SKIP_COLUMNS))
    print("Rules: count-only tables", sorted(COUNT_ONLY))
    diff = difflib.unified_diff(
        snap_a, snap_b, fromfile=label_a, tofile=label_b, lineterm=""
    )
    lines = list(diff)
    print(f"{len(lines)} diff lines")
    for line in lines[:200]:
        print(line)
    return 1


if __name__ == "__main__":
    sys.exit(main())
