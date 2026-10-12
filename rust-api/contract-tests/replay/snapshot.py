# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Snapshot restore for write-replay (PIDASHCONV-823).

The corpus runs against two copies of one seeded snapshot. The
mechanics, per session:

1. Django DB: TRUNCATE every table (RESTART IDENTITY, CASCADE) and load
   the base ``snapshot.sql`` (data-only pg_dump of the migrated +
   seeded template database).
2. Session extras: the session's own seed function runs against the
   Django DB only.
3. Clone: the Django DB's full data is dumped and loaded into the Rust
   DB (after its own TRUNCATE), so both copies are bit-identical without
   constraining session seeds to deterministic SQL.
4. Per-backend rewrites: webhook sink URLs point at each backend's own
   sink; the Rust queue tables are ensured (Rust-owned runtime DDL).

Servers stay up throughout: pooled idle connections hold no locks, so
TRUNCATE + COPY never blocks on them. ``snapshot.sql`` is generated at
setup time (migrate + seed + pg_dump), never committed.
"""

from __future__ import annotations

import subprocess

import psycopg

from . import effects

# --- Rust queue DDL (copies, pinned by test_meta.py) ---
# Verbatim from rust-api/crates/jobs/src/queue.rs ensure_schema_sql() /
# ensure_claim_index_sql() and schedule.rs ensure_schedule_schema_sql().
# serve never runs ensure_schema (worker boot does), so replay ensures
# the tables in the Rust copy itself; the drift guard fails if the Rust
# source changes without this copy following.
QUEUE_TABLE_DDL = (
    "CREATE TABLE IF NOT EXISTS rust_job_queue ("
    "id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,"
    "celery_id TEXT NOT NULL UNIQUE,"
    "task TEXT NOT NULL,"
    "args JSONB NOT NULL DEFAULT '[]',"
    "kwargs JSONB NOT NULL DEFAULT '{}',"
    "queue TEXT NOT NULL DEFAULT 'celery',"
    "status TEXT NOT NULL DEFAULT 'queued',"
    "attempts INTEGER NOT NULL DEFAULT 0,"
    "max_retries INTEGER NOT NULL DEFAULT 3,"
    "visible_at TIMESTAMPTZ NOT NULL DEFAULT now(),"
    "claimed_at TIMESTAMPTZ,"
    "claimed_by TEXT,"
    "created_at TIMESTAMPTZ NOT NULL DEFAULT now(),"
    "last_error TEXT)"
)
QUEUE_INDEX_DDL = (
    "CREATE INDEX IF NOT EXISTS rust_job_queue_claim_idx "
    "ON rust_job_queue (status, visible_at, id)"
)
SCHEDULE_TABLE_DDL = (
    "CREATE TABLE IF NOT EXISTS rust_job_schedule ("
    "name TEXT PRIMARY KEY,"
    "last_run_at TIMESTAMPTZ NOT NULL DEFAULT now())"
)


def list_tables(database_url: str) -> list[str]:
    """Every base table in the public schema, sorted."""
    with psycopg.connect(database_url, autocommit=True) as conn, conn.cursor() as cur:
        cur.execute(
            """
            SELECT tablename FROM pg_tables
            WHERE schemaname = 'public' ORDER BY 1
            """
        )
        return [row[0] for row in cur.fetchall()]


def truncate_all(database_url: str) -> None:
    """Empty every table; identity sequences restart for determinism."""
    tables = list_tables(database_url)
    if not tables:
        return
    quoted = ", ".join(f'"{t}"' for t in tables)
    with psycopg.connect(database_url, autocommit=True) as conn, conn.cursor() as cur:
        cur.execute(f"TRUNCATE {quoted} RESTART IDENTITY CASCADE")


def dump_data(database_url: str, out_path: str) -> None:
    """pg_dump the database's data (COPY format) to ``out_path``."""
    with open(out_path, "wb") as fh:
        proc = subprocess.run(
            [
                "pg_dump",
                "--data-only",
                "--no-owner",
                "--no-privileges",
                "--disable-triggers",
                database_url,
            ],
            stdout=fh,
            stderr=subprocess.PIPE,
            check=False,
        )
    if proc.returncode != 0:
        raise RuntimeError(f"pg_dump failed: {proc.stderr.decode()[-2000:]}")


def load_data(database_url: str, sql_path: str) -> None:
    """Load a pg_dump data file with psql (single transaction)."""
    proc = subprocess.run(
        ["psql", "-v", "ON_ERROR_STOP=1", "-1", "-q", "-f", sql_path, database_url],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"psql load failed: {proc.stderr.decode()[-2000:]}")


def clone_database(source_url: str, target_url: str, work_path: str) -> None:
    """Make ``target``'s data bit-identical to ``source``'s (via dump)."""
    truncate_all(target_url)
    dump_data(source_url, work_path)
    load_data(target_url, work_path)


def ensure_queue_schema(database_url: str) -> None:
    """Create the Rust queue tables (idempotent; Rust copy only)."""
    with psycopg.connect(database_url, autocommit=True) as conn, conn.cursor() as cur:
        cur.execute(QUEUE_TABLE_DDL)
        cur.execute(QUEUE_INDEX_DDL)
        cur.execute(SCHEDULE_TABLE_DDL)


def rewrite_webhook_sinks(database_url: str, sink_base_url: str) -> int:
    """Point seeded webhook rows at one backend's sink. Returns row count."""
    with psycopg.connect(database_url, autocommit=True) as conn, conn.cursor() as cur:
        cur.execute(
            "UPDATE webhooks SET url = replace(url, %s, %s)",
            ("http://" + effects.WEBHOOK_PLACEHOLDER_HOST, sink_base_url.rstrip("/")),
        )
        return cur.rowcount


def main(argv=None) -> int:
    """Setup CLI: seed the template database, then dump the snapshot."""
    import argparse
    import json

    parser = argparse.ArgumentParser(description="Replay snapshot setup")
    sub = parser.add_subparsers(dest="command", required=True)
    seed = sub.add_parser("seed", help="seed the base world into a migrated DB")
    seed.add_argument("--database-url", required=True)
    dump = sub.add_parser("dump", help="pg_dump the data snapshot")
    dump.add_argument("--database-url", required=True)
    dump.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    if args.command == "seed":
        from .corpus.world import seed_world

        world = seed_world(args.database_url)
        print(json.dumps({"users": sorted(world["users"]), "instance": world["instance"]["id"]}))
        return 0
    if args.command == "dump":
        dump_data(args.database_url, args.out)
        return 0
    raise AssertionError(f"unreachable command {args.command}")  # pragma: no cover


if __name__ == "__main__":
    raise SystemExit(main())
