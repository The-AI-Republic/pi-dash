"""V1 move with an open run carrying an unknown stored trigger (PIDASHCONV-1330).

Same contract as the app move: rows left labelled ``blocker_completed``
by Django migrations 0027/0029 (and any other unknown value — the
column is a plain varchar) must not fail the move, and the handoff run
inherits the stored trigger verbatim.
"""

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract


@pytest.mark.parametrize("trigger", ["blocker_completed", "not_a_trigger"])
def test_move_hands_off_run_with_unknown_trigger(
    api, world, seeder, db, trigger,
):
    ws_id = world["workspace"]["id"]
    target = seeder.create_project(ws_id, name="Unknown-trigger target")
    seeder.create_project_member(ws_id, target["id"], world["owner"]["id"], role=20)
    seeder.create_state(ws_id, target["id"])
    tgt_pod = seeder.create_pod(ws_id, target["id"], is_default=True)
    src_pod = seeder.create_pod(ws_id, world["project"]["id"])
    run = seeder.create_agent_run(
        ws_id, world["owner"]["id"], src_pod["id"], world["issue"]["id"],
        status="queued",
    )
    db.execute(
        "UPDATE agent_run SET trigger = %s WHERE id = %s", (trigger, run["id"])
    )
    try:
        response = api.post(
            issue_url(world, world["issue"]["id"]) + "move/",
            json={"project": target["id"]},
        )
        assert response.status_code == 200, response.text
        body = response.json()
        assert body["project"] == target["id"]
        parent = db.fetchone(
            "SELECT status FROM agent_run WHERE id = %s", (run["id"],)
        )
        assert parent["status"] == "cancelled"
        fresh = db.fetchone(
            "SELECT status, pod_id, work_item_id, trigger, prompt"
            " FROM agent_run WHERE parent_run_id = %s",
            (run["id"],),
        )
        assert fresh["status"] == "queued"
        assert str(fresh["pod_id"]) == tgt_pod["id"]
        assert str(fresh["work_item_id"]) == world["issue"]["id"]
        assert fresh["trigger"] == trigger
        assert fresh["prompt"] not in (None, "")
    finally:
        # Server-created side rows are untracked: remove them so teardown
        # leaves no orphans behind (their FKs would otherwise block the
        # tracked parent-run delete).
        db.execute(
            "DELETE FROM agent_run WHERE work_item_id = %s",
            (world["issue"]["id"],),
        )
        db.execute(
            "DELETE FROM issue_sequences WHERE issue_id = %s",
            (world["issue"]["id"],),
        )
