# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared scenario for PIDASHCONV-1232: review continuation parent payload.

Django ``_parent_done_payload`` (``prompting/context.py:401-416``) consults
the ticker's ``resume_parent_run`` only when the run has no parent run of its
own. When a parent run exists but its ``done_payload`` is falsy, Django
renders ``(no parent run done payload available)``.

The Rust port (``resolve_parent_payload``, ``context.rs:660-671``) falls
through to the ticker payload whenever the direct payload is missing or
falsy, because the loader (``creation_jobs.rs:1237-1240``) collapses
"no parent run" and "parent run with NULL payload" into one ``None``.

Scenario: an In Review issue whose implementation run R1 completed with a
done payload, the ticker's ``resume_parent_run`` is R1, and the review run
R2 (child of R1's stage, ``phase_kind="review"``) ended ``failed`` with
``done_payload`` NULL. A tick dispatches the review continuation R3 with
``parent_run=R2``. R3's prompt must carry the fallback line — never R1's
stale payload.

Only the transport differs between backends, so the seed and the assertions
live here once. The Django oracle (``test_review_parent_payload.py``)
publishes to the Celery broker; the Rust replay
(``test_review_parent_payload_rust.py``) seeds ``rust_job_queue``.
"""

import json
from datetime import datetime, timedelta, timezone

from _harness import world
from _harness.db import fetchone, get_database_url, wait_for_condition

FIRE = "pi_dash.bgtasks.agent_ticker.fire_tick"

NO_PARENT_PAYLOAD_TEXT = "(no parent run done payload available)"
R1_FINGERPRINT = "r1-impl-fingerprint-1232"
R1_PAYLOAD = {"review_target": R1_FINGERPRINT, "verdict": "ship"}


def _past(minutes: int = 5) -> str:
    return (datetime.now(timezone.utc) - timedelta(minutes=minutes)).isoformat()


def seed_review_continuation(tag: str) -> tuple[world.World, str, str, str]:
    """An In Review issue with R1 (payload) / R2 (failed, NULL payload).

    Returns ``(world, issue_id, r1_id, r2_id)``. The ticker is due and its
    ``resume_parent_run`` is R1; R2 is the latest run (``phase_kind`` matches
    the review stage, so the tick parents R3 off R2, same-stage). A project
    default pod plus an online runner owned by the issue creator satisfy the
    dispatch preflight on both backends.
    """
    w = world.build(f"1232-{tag}")
    issue = world.make_issue(w, "review-parent", "In Review")
    pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    world.make_online_runner(w.user_id, w.workspace_id, pod)
    r1 = world.make_run(
        issue, w.workspace_id, w.user_id, pod,
        status="completed", phase_kind="coding-task",
        done_payload=json.dumps(R1_PAYLOAD),
    )
    world.set_ticker(
        issue, next_run_at=_past(), resume_parent_run_id=r1,
    )
    r2 = world.make_run(
        issue, w.workspace_id, w.user_id, pod,
        status="failed", phase_kind="review", parent_run_id=r1,
    )
    # ``_latest_prior_run`` orders by ``-created_at``; both rows were just
    # inserted, so pin R1 strictly older to remove the tie.
    world._exec(
        "UPDATE agent_run SET created_at = now() - interval '1 minute' WHERE id = %s",
        (r1,),
    )
    return w, issue, r1, r2


def ticker_id(issue: str) -> str:
    return str(fetchone(
        get_database_url(),
        "SELECT id FROM issue_agent_ticker WHERE issue_id = %s",
        (issue,),
    )["id"])


def runs(issue: str) -> int:
    return fetchone(
        get_database_url(),
        "SELECT count(*) AS n FROM agent_run WHERE work_item_id = %s",
        (issue,),
    )["n"]


def wait_for_continuation(issue: str, timeout: float = 90) -> dict:
    """Wait until the tick's continuation run R3 exists; return its row."""
    wait_for_condition(
        lambda: runs(issue) == 3,
        timeout=timeout,
        what="fire_tick dispatches the review continuation run R3",
    )
    return fetchone(
        get_database_url(),
        "SELECT id, parent_run_id, prompt, status,"
        " COALESCE(error_details ->> 'message', '') AS error,"
        " COALESCE(error_details ->> 'code', '') AS error_code"
        " FROM agent_run WHERE work_item_id = %s ORDER BY created_at DESC LIMIT 1",
        (issue,),
    )


def assert_review_parent_payload(r3: dict, r2_id: str) -> None:
    """R3 is parented off R2 and carries the fallback payload block."""
    assert str(r3["parent_run_id"]) == str(r2_id), r3
    assert r3["error_code"] != "prompt_build_failed", r3
    prompt = r3["prompt"] or ""
    assert NO_PARENT_PAYLOAD_TEXT in prompt, prompt[-2000:]
    assert R1_FINGERPRINT not in prompt, prompt[-2000:]
