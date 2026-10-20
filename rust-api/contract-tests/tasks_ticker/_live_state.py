# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared scenario for PIDASHCONV-1218: a ticking state created after boot.

`fire_tick` must resolve the issue's state *live*, exactly as Django does
with `select_related("issue__state")` (`agent_ticker.py:99-104,185-187`; the
interval via `IssueAgentTicker.effective_interval_seconds`,
`db/models/issue_agent_ticker.py:193-206`). A worker that caches every state
row once at boot never sees a state created afterwards, so every project
created after the last deploy silently stops ticking (and the due row is
re-fired every minute).

This module seeds the scenario and asserts the observable claim. The Django
oracle (`test_ticker_live_state.py`) publishes to the Celery broker; the Rust
replay (`test_rust_replay.py`) seeds `rust_job_queue`. Only the transport
differs, so the seed and the assertions live here once.
"""

from datetime import datetime, timedelta, timezone

from _harness import world
from _harness.db import fetchone, get_database_url, wait_for_condition

FIRE = "pi_dash.bgtasks.agent_ticker.fire_tick"


def _past(minutes: int = 5) -> str:
    return (datetime.now(timezone.utc) - timedelta(minutes=minutes)).isoformat()


def seed_state_after_boot(tag: str) -> tuple[world.World, str]:
    """A fresh project + In Progress state + issue, armed and due.

    Built at test time, i.e. strictly after the worker booted: the state row
    the fix must read live did not exist when the worker loaded its inputs.
    No *default* pod is seeded, so a tick that reaches the claim dispatches
    nothing and rolls the budget back — `last_tick_at` (never rolled back on
    either backend, `agent_ticker.py:292-297`) is the claim's fingerprint.
    """
    w = world.build(f"1218-{tag}")
    issue = world.make_issue(w, "live-state", "In Progress")
    world.set_ticker(issue, next_run_at=_past())
    pod = world.make_pod(w.workspace_id, w.project_id)
    world.make_run(issue, w.workspace_id, w.user_id, pod, status="completed")
    return w, issue


def ticker(issue: str) -> dict:
    return fetchone(
        get_database_url(),
        "SELECT * FROM issue_agent_ticker WHERE issue_id = %s",
        (issue,),
    )


def run_count(issue: str) -> int:
    return fetchone(
        get_database_url(),
        "SELECT count(*) AS n FROM agent_run WHERE work_item_id = %s",
        (issue,),
    )["n"]


def wait_for_claim(issue: str, timeout: float = 45, what: str | None = None) -> dict:
    """Wait until fire_tick stamped `last_tick_at`, then return the row.

    `last_tick_at` is written inside the claim and is deliberately not rolled
    back when dispatch returns nothing (`agent_ticker.py:295-297`), so it is
    the one column that proves the ticking-state gate opened and the claim
    ran — on both backends.
    """
    wait_for_condition(
        lambda: ticker(issue)["last_tick_at"] is not None,
        timeout=timeout,
        what=what or "fire_tick claims the due ticker",
    )
    return ticker(issue)
