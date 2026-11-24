"""Machine capacity gate: the matcher respects the dev-machine limit (NEWPIDASH-398).

The server's runner matcher used to assign one run to every idle
runner, so a machine with more runners than its concurrent-run limit
took more runs than it could execute. The excess waited in the
daemon's local queue even while other machines had free slots.

New-system-only (no Django counterpart): the matcher assigns to a
runner only when its machine's held runs (assigned locally waiting,
preparing, or running — any non-terminal status) are below the
machine's last reported limit. Runners without
``machine_run_queue_v1`` (protocol 0), or with no reported limit,
keep today's behaviour.
"""

from __future__ import annotations

import threading

import pytest

from _harness.auth import login_session
from _harness.http import api_client, bearer_client
from _harness.web import web_post

from .conftest import DAEMON
from .test_engine_dispatch import _go_online, _mk_runner, _queued_run

pytestmark = pytest.mark.contract

WEB_RUNNERS = "/api/runners"


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------


def _flow_machine_id(seeder, daemon_world, machine_flow):
    return seeder.db.fetchval(
        "SELECT id FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], machine_flow["host_label"]),
    )


def _set_limit(seeder, machine_id, limit):
    seeder.db.execute(
        "UPDATE dev_machine SET run_concurrency_limit = %s WHERE id = %s",
        (limit, machine_id),
    )


def _open_queue_session(client, runner_id):
    response = client.post(
        f"{DAEMON}/runners/{runner_id}/sessions/",
        json={"capabilities": ["machine_run_queue_v1"]},
    )
    assert response.status_code == 201, response.text
    return response.json()["session_id"]


def _open_plain_session(client, runner_id):
    response = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()["session_id"]


def _runner_client(settings, machine_token, runner_id):
    return bearer_client(
        settings.base_url, machine_token, headers={"X-Runner-Id": runner_id}
    )


def _owner_client(settings, daemon_world):
    ctx = api_client(settings.base_url)
    client = ctx.__enter__()
    login_session(
        client,
        email=daemon_world["owner"]["email"],
        password=daemon_world["owner"]["password"],
    )
    return ctx, client


def _ticking_issue(seeder, daemon_world):
    state = seeder.create_state(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        name="In Progress",
        group="started",
    )
    issue = seeder.create_issue(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"], state["id"]
    )
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s WHERE id = %s",
        (daemon_world["owner"]["id"], issue["id"]),
    )
    return issue["id"]


def _run_ai(client, issue_id):
    response = web_post(
        client, f"{WEB_RUNNERS}/runs/", {"triggered_by": "run_ai", "work_item": issue_id}
    )
    assert response.status_code in (201, 202), response.text[:500]
    return response.json()


def _assigned_counts(seeder, run_ids):
    rows = seeder.db.fetchall(
        "SELECT id, status, runner_id FROM agent_run WHERE id = ANY(%s)", (run_ids,)
    )
    assigned = [r for r in rows if r["status"] == "assigned"]
    queued = [r for r in rows if r["status"] == "queued"]
    return assigned, queued


def _complete(client, seeder, run_id, session_id):
    import uuid as uuid_mod

    token = seeder.db.fetchval(
        "SELECT assignment_token FROM agent_run WHERE id = %s", (run_id,)
    )
    response = client.post(
        f"{DAEMON}/runs/{run_id}/accept/",
        json={"session_id": session_id, "assignment_token": str(token)},
    )
    assert response.status_code == 200, response.text
    response = client.post(
        f"{DAEMON}/runs/{run_id}/complete/",
        json={"done_payload": {"summary": "done"}},
        headers={"Idempotency-Key": uuid_mod.uuid4().hex},
    )
    assert response.status_code == 200, response.text
    assert response.json() == {"ok": True}, response.text


# ---------------------------------------------------------------------------
# fixture: an isolated machine world with full teardown
# ---------------------------------------------------------------------------


@pytest.fixture()
def mcg(seeder, daemon_world, machine_flow, settings):
    """Seeded runners/machines/runs on the flow machine, fully torn down.

    The flow runner itself is parked offline: it only authenticates.
    Everything this fixture creates is removed in reverse, before the
    seed tracker and ``machine_flow`` teardowns run.
    """
    owner_id = daemon_world["owner"]["id"]
    ws_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    tag = seeder.tag
    machine_id = _flow_machine_id(seeder, daemon_world, machine_flow)
    # Park the flow runner: auth only, never drain-eligible.
    seeder.db.execute(
        "UPDATE runner SET status = 'offline', last_heartbeat_at = NULL WHERE id = %s",
        (machine_flow["runner_id"],),
    )
    state = {
        "machine_id": machine_id,
        "runners": [],
        "sessions": {},
        "machines": [],
        "run_ids": [],
        "issue_ids": [],
        "token_host_labels": [],
    }

    def add_runner(dev_machine_id, *, queue_session=True, client=None):
        runner_id = _mk_runner(
            seeder,
            owner_id,
            ws_id,
            pod_id,
            name=f"mcg-{tag}-{len(state['runners'])}",
            provisioning="manual",
            pod_dispatch="pod_member",
            dev_machine_id=dev_machine_id,
        )
        _go_online(seeder, runner_id)
        state["runners"].append(runner_id)
        if queue_session:
            assert client is not None
            state["sessions"][runner_id] = _open_queue_session(client, runner_id)
        return runner_id

    state["add_runner"] = add_runner

    def add_queued_run():
        if not state.get("member_added"):
            seeder.create_project_member(ws_id, daemon_world["project"]["id"], owner_id)
            state["backlog_state"] = seeder.create_state(
                ws_id, daemon_world["project"]["id"]
            )
            state["member_added"] = True
        issue = seeder.create_issue(
            ws_id, daemon_world["project"]["id"], state["backlog_state"]["id"]
        )
        seeder.db.execute(
            "UPDATE issues SET created_by_id = %s, updated_by_id = %s WHERE id = %s",
            (owner_id, owner_id, issue["id"]),
        )
        issue_id = issue["id"]
        state["issue_ids"].append(issue_id)
        run_id = _queued_run(
            seeder, ws_id, owner_id, pod_id, issue_id, created_by=owner_id
        )
        state["run_ids"].append(run_id)
        return run_id

    state["add_queued_run"] = add_queued_run
    yield state
    # Teardown: child rows first, then runs, sessions, runners, machines.
    if state["run_ids"]:
        seeder.db.execute(
            "DELETE FROM run_message_dedupe WHERE run_id = ANY(%s)", (state["run_ids"],)
        )
        seeder.db.execute(
            "DELETE FROM agent_run_event WHERE agent_run_id = ANY(%s)",
            (state["run_ids"],),
        )
        seeder.db.execute(
            "DELETE FROM agent_run_approval WHERE agent_run_id = ANY(%s)",
            (state["run_ids"],),
        )
    # Runs the dispatches created (untracked by the seeder).
    if state["issue_ids"]:
        extra = seeder.db.fetchall(
            "SELECT id FROM agent_run WHERE work_item_id = ANY(%s)", (state["issue_ids"],)
        )
        extra_ids = [r["id"] for r in extra if r["id"] not in state["run_ids"]]
        for table, col in (
            ("run_message_dedupe", "run_id"),
            ("agent_run_event", "agent_run_id"),
            ("agent_run_approval", "agent_run_id"),
        ):
            if extra_ids:
                seeder.db.execute(
                    f"DELETE FROM {table} WHERE {col} = ANY(%s)", (extra_ids,)
                )
        if extra_ids:
            seeder.db.execute("DELETE FROM agent_run WHERE id = ANY(%s)", (extra_ids,))
    if state["runners"]:
        seeder.db.execute(
            "DELETE FROM runner_session WHERE runner_id = ANY(%s)", (state["runners"],)
        )
        seeder.db.execute(
            "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = NULL "
            "WHERE runner_id = ANY(%s) OR pinned_runner_id = ANY(%s)",
            (state["runners"], state["runners"]),
        )
        seeder.db.execute("DELETE FROM runner WHERE id = ANY(%s)", (state["runners"],))
    for label in state["token_host_labels"]:
        seeder.db.execute(
            "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
            (owner_id, label),
        )
    for mid in state["machines"]:
        seeder.db.execute("DELETE FROM machine_session WHERE dev_machine_id = %s", (mid,))
        seeder.db.execute(
            "DELETE FROM engine_credential WHERE dev_machine_id = %s", (mid,)
        )
        seeder.db.execute("DELETE FROM machine_token WHERE dev_machine_id = %s", (mid,))
        seeder.db.execute("DELETE FROM dev_machine WHERE id = %s", (mid,))


# ---------------------------------------------------------------------------
# tests
# ---------------------------------------------------------------------------


def test_pod_drain_caps_assignments_at_machine_limit(
    settings, seeder, daemon_world, machine_flow, mcg
):
    """Five runners, limit 2, eight queued runs: exactly two assigned.

    After one finishes, terminal effects drain again and exactly one
    more run is assigned.
    """
    machine_id = mcg["machine_id"]
    _set_limit(seeder, machine_id, 2)
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        runners = [mcg["add_runner"](machine_id, client=client) for _ in range(5)]
        for _ in range(7):
            mcg["add_queued_run"]()
        trigger_issue = _ticking_issue(seeder, daemon_world)
    ctx, owner = _owner_client(settings, daemon_world)
    try:
        _run_ai(owner, trigger_issue)
    finally:
        ctx.__exit__(None, None, None)
    mcg["issue_ids"].append(trigger_issue)
    all_ids = [str(i) for i in mcg["run_ids"]] + [
        str(r["id"])
        for r in seeder.db.fetchall(
            "SELECT id FROM agent_run WHERE work_item_id = %s", (trigger_issue,)
        )
    ]
    assert len(all_ids) == 8
    assigned, queued = _assigned_counts(seeder, all_ids)
    assert len(assigned) == 2, assigned
    assert len(queued) == 6, queued
    assert {str(r["runner_id"]) for r in assigned} <= set(runners)

    # One finishes: terminal effects drain the freed slot, exactly one
    # more run is assigned.
    finished = assigned[0]
    with _runner_client(
        settings, machine_flow["machine_token"], str(finished["runner_id"])
    ) as rc:
        _complete(
            rc, seeder, str(finished["id"]), mcg["sessions"][str(finished["runner_id"])]
        )
    assigned2, queued2 = _assigned_counts(seeder, all_ids)
    assert len(queued2) == 5, queued2
    live = [r for r in assigned2 if str(r["id"]) != str(finished["id"])]
    assert len(live) == 2, assigned2
    assert str(finished["id"]) not in {str(r["id"]) for r in live}
    assert len({str(r["id"]) for r in live} - {str(r["id"]) for r in assigned}) == 1


def test_full_machine_skipped_next_run_goes_to_free_machine(
    settings, seeder, daemon_world, machine_flow, mcg
):
    """m1 at 2/2 keeps its idle runner empty; the next run goes to m2."""
    m1 = mcg["machine_id"]
    _set_limit(seeder, m1, 2)
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        busy = [mcg["add_runner"](m1, client=client) for _ in range(2)]
        idle_m1 = mcg["add_runner"](m1, client=client)
    # m1 is full: two non-terminal held runs on its runners.
    for runner_id in busy:
        run = seeder.create_daemon_run(
            daemon_world["owner"]["id"],
            daemon_world["workspace"]["id"],
            daemon_world["pod"]["id"],
            runner_id,
            status="running",
        )
        mcg["run_ids"].append(run["id"])
    # Second machine with a free slot.
    m2 = seeder.create_dev_machine(daemon_world["owner"]["id"])["id"]
    mcg["machines"].append(m2)
    _set_limit(seeder, m2, 5)
    raw = f"mt_{seeder.tag}-m2"
    label = f"mcg-{seeder.tag}-m2"
    seeder.create_machine_token(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        raw,
        host_label=label,
        dev_machine_id=m2,
    )
    mcg["token_host_labels"].append(label)
    with bearer_client(settings.base_url, raw) as client2:
        idle_m2 = mcg["add_runner"](m2, client=client2)
    assert idle_m1 != idle_m2
    first = mcg["add_queued_run"]()
    trigger_issue = _ticking_issue(seeder, daemon_world)
    ctx, owner = _owner_client(settings, daemon_world)
    try:
        _run_ai(owner, trigger_issue)
    finally:
        ctx.__exit__(None, None, None)
    mcg["issue_ids"].append(trigger_issue)
    second = seeder.db.fetchval(
        "SELECT id FROM agent_run WHERE work_item_id = %s", (trigger_issue,)
    )
    for run_id in (first, second):
        row = seeder.db.fetchone(
            "SELECT status, runner_id FROM agent_run WHERE id = %s", (run_id,)
        )
        if str(row["runner_id"]) == idle_m2:
            assert row["status"] == "assigned", row
        else:
            assert row["status"] == "queued", row
            assert row["runner_id"] is None, row
    got_m2 = seeder.db.fetchval(
        "SELECT count(*) FROM agent_run WHERE runner_id = %s AND status = 'assigned'",
        (idle_m2,),
    )
    assert got_m2 == 1
    got_m1_idle = seeder.db.fetchval(
        "SELECT count(*) FROM agent_run WHERE runner_id = %s AND status = 'assigned'",
        (idle_m1,),
    )
    assert got_m1_idle == 0


def test_concurrent_drains_fill_last_slot_once(
    settings, seeder, daemon_world, machine_flow, mcg
):
    """Two simultaneous dispatches on a machine with one free slot
    assign exactly one run: the loser's recount sees the winner's
    committed assignment under the shared machine lock."""
    machine_id = mcg["machine_id"]
    _set_limit(seeder, machine_id, 1)
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        mcg["add_runner"](machine_id, client=client)
        mcg["add_runner"](machine_id, client=client)
    ticking = seeder.create_state(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        name="In Progress",
        group="started",
    )
    issues = []
    for _ in range(2):
        issue = seeder.create_issue(
            daemon_world["workspace"]["id"], daemon_world["project"]["id"], ticking["id"]
        )
        seeder.db.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (daemon_world["owner"]["id"], issue["id"]),
        )
        issues.append(issue["id"])
    mcg["issue_ids"].extend(issues)
    barrier = threading.Barrier(3)
    outcomes = []

    def dispatch(issue_id):
        ctx, owner = _owner_client(settings, daemon_world)
        try:
            barrier.wait(timeout=30)
            resp = web_post(
                owner,
                f"{WEB_RUNNERS}/runs/",
                {"triggered_by": "run_ai", "work_item": issue_id},
            )
            outcomes.append(resp.status_code)
        finally:
            ctx.__exit__(None, None, None)

    threads = [threading.Thread(target=dispatch, args=(i,)) for i in issues]
    for t in threads:
        t.start()
    barrier.wait(timeout=30)
    for t in threads:
        t.join(timeout=60)
    assert all(not t.is_alive() for t in threads), "dispatch hung"
    assert sorted(outcomes) == [201, 201], outcomes
    rows = seeder.db.fetchall(
        "SELECT status FROM agent_run WHERE work_item_id = ANY(%s)", (issues,)
    )
    assert sorted(r["status"] for r in rows) == ["assigned", "queued"], rows


def test_protocol_zero_runners_keep_today_behaviour(
    settings, seeder, daemon_world, machine_flow, mcg
):
    """Sessions without the queue capability ignore the machine limit."""
    machine_id = mcg["machine_id"]
    _set_limit(seeder, machine_id, 2)
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        runners = []
        for _ in range(3):
            runner_id = _mk_runner(
                seeder,
                daemon_world["owner"]["id"],
                daemon_world["workspace"]["id"],
                daemon_world["pod"]["id"],
                name=f"mcg-{seeder.tag}-p0-{len(runners)}",
                provisioning="manual",
                pod_dispatch="pod_member",
                dev_machine_id=machine_id,
            )
            _go_online(seeder, runner_id)
            mcg["runners"].append(runner_id)
            _open_plain_session(client, runner_id)
            runners.append(runner_id)
        for _ in range(3):
            mcg["add_queued_run"]()
        trigger_issue = _ticking_issue(seeder, daemon_world)
    ctx, owner = _owner_client(settings, daemon_world)
    try:
        _run_ai(owner, trigger_issue)
    finally:
        ctx.__exit__(None, None, None)
    mcg["issue_ids"].append(trigger_issue)
    count = seeder.db.fetchval(
        "SELECT count(*) FROM agent_run WHERE runner_id = ANY(%s) AND status = 'assigned'",
        (runners,),
    )
    # Today's behaviour: every idle runner takes a run even though the
    # machine limit is 2 — the limit is only enforced for
    # queue-capable runners. The fourth run stays queued: no idle
    # runner remains.
    assert count == 3, count
    left = seeder.db.fetchval(
        "SELECT count(*) FROM agent_run WHERE work_item_id = ANY(%s) AND status = 'queued'",
        (mcg["issue_ids"],),
    )
    assert left == 1, left


def test_capacity_report_persists_machine_limit(
    settings, seeder, daemon_world, machine_flow, machine_client, mcg
):
    """A validated capacity ``limit`` is stored on the dev machine."""
    import uuid as uuid_mod

    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    mcg["run_ids"].append(run["id"])
    token = str(uuid_mod.uuid4())
    seeder.db.execute(
        "UPDATE agent_run SET assignment_token = %s, assignment_protocol = 1,"
        " capacity_wait = NULL WHERE id = %s",
        (token, run["id"]),
    )
    session_id = _open_queue_session(machine_client, machine_flow["runner_id"])
    machine_id = mcg["machine_id"]
    for report, stored in ((3, 3), (5, 5)):
        response = machine_client.post(
            f"{DAEMON}/runs/{run['id']}/capacity/",
            json={
                "action": "waiting",
                "session_id": session_id,
                "assignment_token": token,
                "revision": report,
                "limit": report,
            },
            headers={"Idempotency-Key": str(uuid_mod.uuid4())},
        )
        assert response.status_code == 200, response.text
        assert response.json()["decision"] == "recorded", response.text
        assert (
            seeder.db.fetchval(
                "SELECT run_concurrency_limit FROM dev_machine WHERE id = %s",
                (machine_id,),
            )
            == stored
        )
    # An invalid limit is rejected and changes nothing.
    response = machine_client.post(
        f"{DAEMON}/runs/{run['id']}/capacity/",
        json={
            "action": "waiting",
            "session_id": session_id,
            "assignment_token": token,
            "revision": 9,
            "limit": 0,
        },
        headers={"Idempotency-Key": str(uuid_mod.uuid4())},
    )
    assert response.status_code == 400, response.text
    assert (
        seeder.db.fetchval(
            "SELECT run_concurrency_limit FROM dev_machine WHERE id = %s", (machine_id,)
        )
        == 5
    )


def test_poll_limit_raise_drains_machine(
    settings, seeder, daemon_world, machine_flow, machine_client, mcg
):
    """A poll that raises the persisted limit drains the idle runners."""
    import uuid as uuid_mod

    machine_id = mcg["machine_id"]
    _set_limit(seeder, machine_id, 2)
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    mcg["run_ids"].append(run["id"])
    token = str(uuid_mod.uuid4())
    seeder.db.execute(
        "UPDATE agent_run SET assignment_token = %s, assignment_protocol = 1,"
        " capacity_wait = NULL WHERE id = %s",
        (token, run["id"]),
    )
    session_id = _open_queue_session(machine_client, machine_flow["runner_id"])
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        idle = mcg["add_runner"](machine_id, client=client)
    queued = mcg["add_queued_run"]()
    response = machine_client.post(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{session_id}/poll",
        json={
            "ack": [],
            "status": {
                "status": "busy",
                "in_flight_run": str(run["id"]),
                "held_assignment_token": token,
                "capacity": {
                    "phase": "waiting_for_capacity",
                    "revision": 1,
                    "active": 1,
                    "limit": 5,
                },
            },
        },
    )
    assert response.status_code == 200, response.text
    assert (
        seeder.db.fetchval(
            "SELECT run_concurrency_limit FROM dev_machine WHERE id = %s", (machine_id,)
        )
        == 5
    )
    row = seeder.db.fetchone(
        "SELECT status, runner_id FROM agent_run WHERE id = %s", (queued,)
    )
    assert row["status"] == "assigned", row
    assert str(row["runner_id"]) == idle, row
