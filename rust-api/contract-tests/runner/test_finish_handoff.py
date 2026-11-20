# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-338 item 1: the stored hand-off survives the run's completion.

``pidash run finish --handoff-file`` stores the hand-off as
``agent_run.done_payload.stage_handoff``; when the run then ends, the
runner's COMPLETED frame replaces ``done_payload``. The finalize merge
must keep the finish keys, or the review/test entry run finds no
hand-off (and its prompt renders no hand-off block).

New-system-only: Django has no ``finish`` endpoint (404). On Rust the
test runs the full chain — finish with a hand-off, COMPLETED finalize,
then the hand-off query the stage-inputs loader runs for the review
run — and asserts the hand-off comes back.
"""

from __future__ import annotations

import uuid

import httpx
import pytest

from _harness.diffs import is_rust

from .conftest import DAEMON

pytestmark = pytest.mark.contract

HANDOFF = "### Hand-off\n\n**Produced:** the contract PR.\n"
COMMENT = "Done: moved to review; the hand-off carries the details."


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
def handoff_world(seeder, daemon_world, machine_flow):
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
    issue = seeder.create_issue(workspace_id, project_id, states["progress"], name="handoff chain")
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


def test_finish_handoff_survives_completion(
    settings, seeder, daemon_world, machine_flow, machine_client, handoff_world
):
    """Finish stores the hand-off; COMPLETED keeps it; the review query finds it."""
    slug = daemon_world["workspace"]["slug"]
    run_id = handoff_world["run"]["id"]
    issue_id = handoff_world["issue"]["id"]
    with _v1(settings, daemon_world) as client:
        resp = client.post(
            f"/api/v1/workspaces/{slug}/agent-runs/{run_id}/finish/",
            json={
                "to": "In Review",
                "result": "done",
                "comment": COMMENT,
                "handoff": HANDOFF,
            },
            headers={"X-Pi-Dash-Run-Id": run_id},
        )
    if not is_rust():
        assert resp.status_code == 404, resp.text[:500]
        return
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["state"] == "In Review", resp.text[:500]
    bodies = seeder.db.fetchall(
        "SELECT comment_stripped FROM issue_comments WHERE issue_id = %s",
        (issue_id,),
    )
    texts = [row["comment_stripped"] for row in bodies]
    assert any(COMMENT in text for text in texts), texts
    assert any("contract PR" in text for text in texts), texts
    row = seeder.db.fetchone("SELECT state_id FROM issues WHERE id = %s", (issue_id,))
    assert str(row["state_id"]) == handoff_world["states"]["review"], row

    machine_client.post(f"{DAEMON}/runs/{run_id}/accept/", json={})
    resp = machine_client.post(
        f"{DAEMON}/runs/{run_id}/complete/",
        json={"done_payload": {}},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert resp.status_code == 200, resp.text
    assert resp.json() == {"ok": True}, resp.text

    payload = seeder.db.fetchone(
        "SELECT done_payload FROM agent_run WHERE id = %s", (run_id,)
    )["done_payload"]
    assert payload["stage_handoff"] == HANDOFF, payload
    assert payload.get("finish_comment_id"), payload
    assert payload.get("handoff_comment_id"), payload

    # What `load_stage_inputs` runs for the review run (BUNDLE_HANDOFF_SQL).
    found = seeder.db.fetchone(
        """SELECT done_payload->>'stage_handoff' AS handoff FROM agent_run
            WHERE work_item_id = %s AND phase_kind = 'coding-task'
              AND done_payload->>'stage_handoff' IS NOT NULL
            ORDER BY created_at DESC LIMIT 1""",
        (issue_id,),
    )["handoff"]
    assert found == HANDOFF, found
