# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unified workpad (``.ai_design/unified_workpad/design.md`` §8): the stage
hand-off is gone and the workpad is the one cross-run memory.

- A coding-task run moving the issue to review or test must have updated
  the workpad since the run was created (the run's stamped
  ``run_config.workpad_sha256``); otherwise ``finish`` answers 400 and
  nothing moves.
- During the compatibility window a legacy ``handoff`` (a prompt rendered
  before the change) still finishes: it is posted as a comment, as before,
  but stored nowhere — no ``stage_handoff`` survives completion.

New-system-only: Django has no ``finish`` endpoint (404).
"""

from __future__ import annotations

import hashlib
import json
import uuid

import httpx
import pytest

from _harness.diffs import is_rust

from .conftest import DAEMON

pytestmark = pytest.mark.contract

HANDOFF = "### Hand-off\n\n**Produced:** the contract PR.\n"
COMMENT = "Done: opened the contract PR and moved it to review."
UNCHANGED = "update the workpad before moving to review or test"


def _make_state(seeder, workspace_id, project_id, name, group):
    state_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO states
               (id, workspace_id, project_id, name, description, color, slug,
                sequence, "group", is_triage, "default", created_at, updated_at)
           VALUES (%s,%s,%s,%s,'','#808080',%s,65535,%s,false,false,now(),now())""",
        (state_id, workspace_id, project_id, name, name.lower().replace(" ", "-"), group),
    )
    seeder._put("states", state_id)
    return state_id


def _delete_run_rows(seeder, run_id):
    seeder.db.execute("DELETE FROM run_message_dedupe WHERE run_id = %s", (run_id,))
    seeder.db.execute("DELETE FROM agent_run_event WHERE agent_run_id = %s", (run_id,))
    seeder.db.execute("DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run_id,))
    seeder.db.execute("DELETE FROM agent_run WHERE id = %s", (run_id,))


@pytest.fixture()
def workpad_world(seeder, daemon_world, machine_flow):
    """Issue in In Progress with the flow runner's daemon run on it."""
    workspace_id = daemon_world["workspace"]["id"]
    project_id = daemon_world["project"]["id"]
    owner_id = daemon_world["owner"]["id"]
    seeder.create_project_member(workspace_id, project_id, owner_id, role=20)
    states = {
        "progress": _make_state(seeder, workspace_id, project_id, "In Progress", "started"),
        "review": _make_state(seeder, workspace_id, project_id, "In Review", "review"),
        "completed": _make_state(seeder, workspace_id, project_id, "AI Completed", "completed"),
    }
    issue = seeder.create_issue(workspace_id, project_id, states["progress"], name="workpad chain")
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s WHERE id = %s",
        (owner_id, issue["id"]),
    )
    run = seeder.create_daemon_run(
        owner_id,
        workspace_id,
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s, phase_kind = 'coding-task' WHERE id = %s",
        (issue["id"], run["id"]),
    )
    yield {"issue": issue, "run": run, "states": states, "project_id": project_id}
    # The entry run arms a ticker pointing back at the finished run
    # (resume_parent_run_id): drop the ticker first.
    seeder.db.execute("DELETE FROM issue_agent_ticker WHERE issue_id = %s", (issue["id"],))
    for row in seeder.db.fetchall(
        "SELECT id FROM agent_run WHERE parent_run_id = %s", (run["id"],)
    ):
        _delete_run_rows(seeder, row["id"])
    for row in seeder.db.fetchall(
        "SELECT id FROM agent_run WHERE work_item_id = %s", (issue["id"],)
    ):
        _delete_run_rows(seeder, row["id"])
    seeder.db.execute("DELETE FROM issue_comments WHERE issue_id = %s", (issue["id"],))


def _v1(settings, daemon_world):
    token = daemon_world["api_token"]
    raw = token["token"] if isinstance(token, dict) else token
    return httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": raw}
    )


def _finish(settings, daemon_world, slug, run_id, body):
    with _v1(settings, daemon_world) as client:
        return client.post(
            f"/api/v1/workspaces/{slug}/agent-runs/{run_id}/finish/",
            json=body,
            headers={"X-Pi-Dash-Run-Id": run_id},
        )


def _stamp_workpad_hash(seeder, run_id, body):
    """What run creation stamps (NEWPIDASH-320): the workpad's hash then."""
    digest = hashlib.sha256(body.encode()).hexdigest()
    seeder.db.execute(
        "UPDATE agent_run SET run_config = COALESCE(run_config, '{}'::jsonb) || %s::jsonb "
        "WHERE id = %s",
        (json.dumps({"workpad_sha256": digest}), run_id),
    )


def test_finish_to_review_needs_a_workpad_update(
    settings, seeder, daemon_world, machine_flow, workpad_world
):
    """Unchanged workpad: refused, issue stays. Updated workpad: moves."""
    slug = daemon_world["workspace"]["slug"]
    run_id = workpad_world["run"]["id"]
    issue_id = workpad_world["issue"]["id"]
    _stamp_workpad_hash(seeder, run_id, "")

    resp = _finish(
        settings, daemon_world, slug, run_id,
        {"to": "In Review", "result": "done", "comment": COMMENT},
    )
    if not is_rust():
        assert resp.status_code == 404, resp.text[:500]
        return
    assert resp.status_code == 400, resp.text[:500]
    assert UNCHANGED in resp.json()["error"], resp.text[:500]
    row = seeder.db.fetchone("SELECT state_id FROM issues WHERE id = %s", (issue_id,))
    assert str(row["state_id"]) == workpad_world["states"]["progress"], row
    assert not seeder.db.fetchall(
        "SELECT id FROM issue_comments WHERE issue_id = %s", (issue_id,)
    ), "a refused finish posts nothing"

    seeder.db.execute(
        "UPDATE issues SET workpad = %s WHERE id = %s",
        ("### Keep in mind\n- The contract PR; check with `make test`.\n", issue_id),
    )
    resp = _finish(
        settings, daemon_world, slug, run_id,
        {"to": "In Review", "result": "done", "comment": COMMENT},
    )
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["state"] == "In Review", resp.text[:500]
    payload = seeder.db.fetchone(
        "SELECT done_payload FROM agent_run WHERE id = %s", (run_id,)
    )["done_payload"] or {}
    assert "stage_handoff" not in payload, payload
    assert "pending_stage_handoff" not in payload, payload


def test_finish_with_a_legacy_handoff_posts_it_and_stores_nothing(
    settings, seeder, daemon_world, machine_flow, machine_client, workpad_world
):
    """An old prompt's hand-off passes the gate, is posted as a comment,
    dedupes on retry, and leaves no ``stage_handoff`` after completion."""
    slug = daemon_world["workspace"]["slug"]
    run_id = workpad_world["run"]["id"]
    issue_id = workpad_world["issue"]["id"]
    _stamp_workpad_hash(seeder, run_id, "")
    body = {"to": "In Review", "result": "done", "comment": COMMENT, "handoff": HANDOFF}

    resp = _finish(settings, daemon_world, slug, run_id, body)
    if not is_rust():
        assert resp.status_code == 404, resp.text[:500]
        return
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["state"] == "In Review", resp.text[:500]
    texts = [
        row["comment_stripped"]
        for row in seeder.db.fetchall(
            "SELECT comment_stripped FROM issue_comments WHERE issue_id = %s", (issue_id,)
        )
    ]
    assert any(COMMENT in text for text in texts), texts
    assert any("contract PR" in text for text in texts), texts

    # A retry dedupes through the comment operation's external id
    # (NEWPIDASH-338 item 6). It runs before COMPLETED: an ended run is no
    # longer active and `finish` refuses.
    retry = _finish(settings, daemon_world, slug, run_id, body)
    assert retry.status_code == 200, retry.text[:500]
    rows = seeder.db.fetchall(
        "SELECT external_source, external_id FROM issue_comments WHERE issue_id = %s",
        (issue_id,),
    )
    assert len(rows) == 2, rows
    assert {row["external_source"] for row in rows} == {"run-finish"}, rows

    machine_client.post(f"{DAEMON}/runs/{run_id}/accept/", json={})
    resp = machine_client.post(
        f"{DAEMON}/runs/{run_id}/complete/",
        json={"done_payload": {}},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert resp.status_code == 200, resp.text
    payload = seeder.db.fetchone(
        "SELECT done_payload FROM agent_run WHERE id = %s", (run_id,)
    )["done_payload"] or {}
    assert "stage_handoff" not in payload, payload
    assert "pending_stage_handoff" not in payload, payload
