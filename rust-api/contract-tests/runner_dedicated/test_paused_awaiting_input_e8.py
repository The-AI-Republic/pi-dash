"""Dedicated-runner E8 check: paused_awaiting_input passes (PIDASHCONV-1734).

Django `apps/api/pi_dash/runner/services/issue_assignment.py:184-197` and
`:524-541` build the blocking set as NON_TERMINAL_STATUSES minus both
QUEUED and PAUSED_AWAITING_INPUT. Rust must match: a paused_awaiting_input
run does not block assign/change/clear, while running still refuses with
active_run_in_progress.

Seeds two admin-owned runners in the issue pod, one paused run (and, in the
control, one running run), then asserts assign, clear, and the
assignable_for read.
"""

import uuid


def _issue_url(world, issue_id):
    return f"/api/workspaces/{world['ws']['slug']}/projects/{world['project']['id']}/issues/{issue_id}/"


def _runners_url(world, issue_id):
    return f"/api/runners/?project={world['project']['id']}&assignable_for={issue_id}"


def _uid():
    return str(uuid.uuid4())


def _make_online_runner(seeder, world, token):
    runner = seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], world["pod"]["id"], token
    )
    seeder.db.execute(
        "UPDATE runner SET status='online', last_heartbeat_at=now() WHERE id=%s",
        (runner["id"],),
    )
    return runner


def _insert_run(seeder, world, issue_id, status):
    run_id = _uid()
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, status, prompt, run_config, required_capabilities, thread_id,
            error_details, workspace_id, work_item_id, pod_id, created_by_id,
            llm_model, trigger, executor_kind, dispatch_attempts, cancel_reason,
            tool_plan, phase_kind, agent_metadata, usage, pinned_runner_id,
            pin_kind, created_at)
           VALUES (%s,%s,'e8 prompt','{}','{}','',
                   '{}',%s,%s,%s,%s,'model','human','local_runner',0,'',
                   '{}','work','{}','{"input": 0, "output": 0, "total": 0}',
                   NULL,NULL,now())""",
        (
            run_id,
            status,
            world["ws"]["id"],
            issue_id,
            world["pod"]["id"],
            world["admin"]["id"],
        ),
    )
    seeder.track.add("agent_run", run_id)
    return {"id": run_id}


def test_paused_awaiting_input_does_not_block_assign_clear_or_picker(
    admin_client, world, seeder
):
    """Steps 3-4: assign, clear, and assignable_for pass with only a
    paused_awaiting_input run (Django behavior, PIDASHCONV-1734)."""
    runner_a = _make_online_runner(seeder, world, f"e8-a-{seeder.tag}")
    runner_b = _make_online_runner(seeder, world, f"e8-b-{seeder.tag}")
    issue_id = world["issue2"]["id"]
    _insert_run(seeder, world, issue_id, "paused_awaiting_input")

    # Step 3a: assign
    resp = admin_client.patch(
        _issue_url(world, issue_id),
        json={"assigned_runner_id": runner_a["id"]},
    )
    assert resp.status_code in (200, 204), resp.text[:1000]

    # Step 3b: clear (with runner assigned)
    resp = admin_client.patch(
        _issue_url(world, issue_id),
        json={"assigned_runner_id": None},
    )
    assert resp.status_code in (200, 204), resp.text[:1000]

    # Step 4: picker reports both assignable
    resp = admin_client.get(_runners_url(world, issue_id))
    assert resp.status_code == 200, resp.text[:1000]
    rows = {str(r["id"]): r for r in resp.json()}
    for runner in (runner_a, runner_b):
        row = rows[str(runner["id"])]
        assert row.get("assignable") is True, row
        assert row.get("not_assignable_code") == "", row


def test_running_run_still_blocks_assign_clear_and_picker(
    admin_client, world, seeder
):
    """Control: a running run refuses with active_run_in_progress on both
    backends (E8 still blocks)."""
    runner_a = _make_online_runner(seeder, world, f"e8-c-a-{seeder.tag}")
    runner_b = _make_online_runner(seeder, world, f"e8-c-b-{seeder.tag}")
    issue_id = world["issue2"]["id"]
    _insert_run(seeder, world, issue_id, "running")

    resp = admin_client.patch(
        _issue_url(world, issue_id),
        json={"assigned_runner_id": runner_a["id"]},
    )
    assert resp.status_code == 400, resp.text[:1000]
    assert "the issue has an active run" in resp.text

    # Clear is also blocked when a runner is already assigned and a
    # running run exists (separate fixture: assign via DB, then clear).
    seeder.db.execute(
        "UPDATE issues SET assigned_runner_id=%s WHERE id=%s",
        (runner_a["id"], issue_id),
    )
    resp = admin_client.patch(
        _issue_url(world, issue_id),
        json={"assigned_runner_id": None},
    )
    assert resp.status_code == 400, resp.text[:1000]
    assert "the issue has an active run" in resp.text

    resp = admin_client.get(_runners_url(world, issue_id))
    assert resp.status_code == 200, resp.text[:1000]
    rows = {str(r["id"]): r for r in resp.json()}
    for runner in (runner_a, runner_b):
        row = rows[str(runner["id"])]
        assert row.get("assignable") is False, row
        assert row.get("not_assignable_code") == "active_run_in_progress", row
