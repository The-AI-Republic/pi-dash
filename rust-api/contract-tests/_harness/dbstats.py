# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-request statement counting via ``pg_stat_statements`` (PIDASHCONV-1266).

``pg_stat_statements`` counters update synchronously at query completion,
so a delta around one HTTP request attributes exactly. (``pg_stat_*``
table counters cannot do this: pooled backends flush them seconds late,
and they also count correlated-subquery executions, which grow with the
row count by design on both backends.)

Requires the extension preloaded (``shared_preload_libraries``) and
created in the backend database. Stock containers without the preload
cannot provide it, so callers skip the measurement when :func:`available`
is false instead of failing.
"""

from __future__ import annotations

from typing import Any

_SCOPE = "WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())"


def available(db: Any) -> bool:
    """True when statement counting works on the backend database.

    Creates the extension when it is preloaded but not yet created;
    anything else (stock postgres without the preload) means the
    measurement is impossible and the caller must skip it.
    """
    try:
        db.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
    except Exception:
        return False
    try:
        db.fetchone(f"SELECT count(*) AS n FROM pg_stat_statements {_SCOPE} LIMIT 1")
    except Exception:
        return False
    return True


def snapshot(db: Any) -> dict[str, int]:
    """Normalized-query -> calls for the backend database, right now."""
    rows = db.fetchall(f"SELECT query, calls FROM pg_stat_statements {_SCOPE}")
    return {row["query"]: row["calls"] for row in rows}


def delta(db: Any, before: dict[str, int]) -> dict[str, int]:
    """Per-shape call counts accrued since ``before`` (probe's own
    snapshots excluded)."""
    after = snapshot(db)
    out: dict[str, int] = {}
    for query, calls in after.items():
        grown = calls - before.get(query, 0)
        if grown > 0 and "pg_stat_statements" not in query:
            out[query] = grown
    return out
