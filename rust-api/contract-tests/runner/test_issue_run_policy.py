"""Contract tests: "Who can run issues" run-control/trigger gates (NEWPIDASH-261, D3).

The runner runs (`/api/runners/runs/`) follow the project's
`issue_run_policy`: default mode allows only the creator and current
assignees; `project_members` allows any Member/Admin. Django has no
such gate (the setting is Rust-only; Django stores the bag
unvalidated), so the member-denied cases pin both behaviours via
DIFF-53: Django proceeds (201/200), Rust answers 403
`not_issue_runner` and writes nothing. Covers cancel, release-pin,
direct creation, app Run AI (`triggered_by=run_ai`) and Comment & Run
(`triggered_by=comment_and_run`), plus the pod_once carve-out (D1):
in `project_members` a non-creator Member may use pod_once on a
dedicated-runner issue.
"""

import pytest

from _harness.auth import login_session
from _harness.diffs import expect, is_rust
from _harness.http import api_client
from _harness.web import web_post

pytestmark = pytest.mark.contract

WEB = "/api/runners"

REASON = "not_issue_runner"
MESSAGE = (
    "Only the issue's creator and assignees can do this in this project. "
    "Ask the creator to assign you, or ask a project admin to change the "
    "project's 'Who can run issues' setting."
)


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
    return issue


def _set_policy(seeder, daemon_world, value):
    seeder.db.execute(
        "UPDATE projects SET settings = %s::jsonb WHERE id = %s",
        (f'{{"issue_run_policy": "{value}"}}', daemon_world["project"]["id"]),
    )


def _member(seeder, daemon_world, role=15):
    user = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], user["id"])
    seeder.create_project_member(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        user["id"],
        role=role,
    )
    return user


def _member_client(settings, user):
    client_ctx = api_client(settings.base_url)
    client = client_ctx.__enter__()
    login_session(
        client, email=user["email"], password=user["password"]
    )
    return client_ctx, client


def _assign(seeder, daemon_world, issue_id, user_id):
    # Direct insert into issue_assignees (live row) with required FKs.
    import uuid

    row_id = str(uuid.uuid4())
    seeder.db.execute(
        "INSERT INTO issue_assignees (created_at, updated_at, id, assignee_id,"
        " created_by_id, issue_id, project_id, updated_by_id, workspace_id)"
        " VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
        (
            row_id,
            user_id,
            daemon_world["owner"]["id"],
            issue_id,
            daemon_world["project"]["id"],
            daemon_world["owner"]["id"],
            daemon_world["workspace"]["id"],
        ),
    )
    seeder.track.add("issue_assignees", row_id)


def _set_creator(seeder, issue_id, user_id):
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s WHERE id = %s", (user_id, issue_id)
    )


def _runs_table_count(seeder, ws_id):
    return seeder.db.fetchval(
        "SELECT count(*) FROM agent_run WHERE workspace_id = %s", (ws_id,)
    )


@pytest.fixture(autouse=True)
def _delete_created_runs(seeder):
    yield
    ws_ids = seeder.track._tracked_ids("workspaces")
    if not ws_ids:
        return
    db = seeder.db
    db.execute(
        "UPDATE scheduler_bindings SET last_run_id = NULL WHERE last_run_id IN "
        "(SELECT id FROM agent_run WHERE workspace_id = ANY(%s))",
        (ws_ids,),
    )
    db.execute(
        "UPDATE issue_agent_ticker SET resume_parent_run_id = NULL "
        "WHERE resume_parent_run_id IN "
        "(SELECT id FROM agent_run WHERE workspace_id = ANY(%s))",
        (ws_ids,),
    )
    for table, column in (
        ("run_message_dedupe", "run_id"),
        ("agent_run_event", "agent_run_id"),
        ("agent_run_approval", "agent_run_id"),
        ("agent_run_tool_call", "agent_run_id"),
    ):
        db.execute(
            f"DELETE FROM {table} WHERE {column} IN "
            "(SELECT id FROM agent_run WHERE workspace_id = ANY(%s))",
            (ws_ids,),
        )
    db.execute(
        "DELETE FROM agent_run WHERE workspace_id = ANY(%s) AND parent_run_id IS NOT NULL",
        (ws_ids,),
    )
    db.execute("DELETE FROM agent_run WHERE workspace_id = ANY(%s)", (ws_ids,))


# ---------------------------------------------------------------------------
# Cancel
# ---------------------------------------------------------------------------


def test_cancel_denied_for_other_member(settings, seeder, daemon_world, machine_flow):
    # DIFF-53: Django cancels (200), Rust refuses (403, whole change).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    run = seeder.create_agent_run(
        daemon_world["workspace"]["id"],
        daemon_world["owner"]["id"],
        daemon_world["pod"]["id"],
        issue["id"],
        status="running",
    )
    # Give the run a runner so cancel has a visible effect on Django.
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = %s WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(client, f"{WEB}/runs/{run['id']}/cancel/", {})
        assert resp.status_code == expect("DIFF-53", django=200, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            status = seeder.db.fetchval(
                "SELECT status FROM agent_run WHERE id = %s", (run["id"],)
            )
            assert status == "running"
    finally:
        ctx.__exit__(None, None, None)


def test_cancel_allowed_for_creator(settings, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    run = seeder.create_agent_run(
        daemon_world["workspace"]["id"],
        daemon_world["owner"]["id"],
        daemon_world["pod"]["id"],
        issue["id"],
        status="running",
    )
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = %s WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    # Owner is creator -> allowed on both.
    from _harness.auth import login_session

    with api_client(settings.base_url) as client:
        login_session(
            client,
            email=daemon_world["owner"]["email"],
            password=daemon_world["owner"]["password"],
        )
        resp = web_post(client, f"{WEB}/runs/{run['id']}/cancel/", {})
        assert resp.status_code == 200, resp.text[:500]


def test_cancel_allowed_in_project_members(settings, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    run = seeder.create_agent_run(
        daemon_world["workspace"]["id"],
        daemon_world["owner"]["id"],
        daemon_world["pod"]["id"],
        issue["id"],
        status="running",
    )
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = %s WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(client, f"{WEB}/runs/{run['id']}/cancel/", {})
        assert resp.status_code == 200, resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


# ---------------------------------------------------------------------------
# Release-pin
# ---------------------------------------------------------------------------


def test_release_denied_for_other_member(settings, seeder, daemon_world, machine_flow):
    # DIFF-53: Django releases (200), Rust refuses (403, nothing written).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s, pinned_runner_id = %s WHERE id = %s",
        (issue["id"], machine_flow["runner_id"], run["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(client, f"{WEB}/runs/{run['id']}/release-pin/", {})
        assert resp.status_code == expect("DIFF-53", django=200, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            pinned = seeder.db.fetchval(
                "SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run["id"],)
            )
            assert pinned is not None
    finally:
        ctx.__exit__(None, None, None)


def test_release_allowed_in_project_members(settings, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s, pinned_runner_id = %s WHERE id = %s",
        (issue["id"], machine_flow["runner_id"], run["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(client, f"{WEB}/runs/{run['id']}/release-pin/", {})
        assert resp.status_code == 200, resp.text[:500]
        assert (
            seeder.db.fetchval(
                "SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run["id"],)
            )
            is None
        )
    finally:
        ctx.__exit__(None, None, None)


# ---------------------------------------------------------------------------
# Direct creation
# ---------------------------------------------------------------------------


def test_direct_denied_for_other_member(settings, seeder, daemon_world):
    # DIFF-53: Django creates (201), Rust refuses (403, no run).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        before = _runs_table_count(seeder, daemon_world["workspace"]["id"])
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello issue",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
            },
        )
        assert resp.status_code == expect("DIFF-53", django=201, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            assert (
                _runs_table_count(seeder, daemon_world["workspace"]["id"]) == before
            )
    finally:
        ctx.__exit__(None, None, None)


def test_direct_allowed_for_assignee(settings, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    member = _member(seeder, daemon_world, role=15)
    _assign(seeder, daemon_world, issue["id"], member["id"])
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello issue",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
            },
        )
        assert resp.status_code == 201, resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


def test_direct_allowed_in_project_members(settings, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello issue",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
            },
        )
        assert resp.status_code == 201, resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


def test_direct_guest_denied(settings, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    guest = _member(seeder, daemon_world, role=5)
    ctx, client = _member_client(settings, guest)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello guest",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
            },
        )
        assert resp.status_code == 403, resp.text[:500]
        if is_rust():
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
    finally:
        ctx.__exit__(None, None, None)


# ---------------------------------------------------------------------------
# App Run AI / Comment & Run (triggered_by)
# ---------------------------------------------------------------------------


def test_run_ai_denied_for_other_member(settings, seeder, daemon_world):
    # DIFF-53: Django dispatches (201/202/409), Rust refuses (403, no run).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        before = _runs_table_count(seeder, daemon_world["workspace"]["id"])
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {"triggered_by": "run_ai", "work_item": issue["id"]},
        )
        if is_rust():
            assert resp.status_code == 403, resp.text[:500]
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
            assert (
                _runs_table_count(seeder, daemon_world["workspace"]["id"]) == before
            )
        else:
            # DIFF-53: Django has no such refusal; it proceeds.
            assert resp.status_code in (201, 202, 409), resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


def test_run_ai_allowed_for_assignee(settings, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    member = _member(seeder, daemon_world, role=15)
    _assign(seeder, daemon_world, issue["id"], member["id"])
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {"triggered_by": "run_ai", "work_item": issue["id"]},
        )
        assert resp.status_code in (201, 202), resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


def test_comment_and_run_denied_for_other_member(settings, seeder, daemon_world):
    # DIFF-53: Django proceeds, Rust refuses (403, no run, no comment).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {"triggered_by": "comment_and_run", "work_item": issue["id"]},
        )
        if is_rust():
            assert resp.status_code == 403, resp.text[:500]
            assert resp.json() == {"error": MESSAGE, "reason": REASON}
        else:
            assert resp.status_code in (201, 202, 409), resp.text[:500]
    finally:
        ctx.__exit__(None, None, None)


def test_comment_and_run_allowed_in_project_members(
    settings, seeder, daemon_world
):
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {"triggered_by": "comment_and_run", "work_item": issue["id"]},
        )
        # Proceeds past the gate (201/202/409 depending on prior runs).
        assert resp.status_code in (201, 202, 409), resp.text[:500]
        if is_rust():
            assert resp.json().get("reason") != REASON
    finally:
        ctx.__exit__(None, None, None)


def test_pod_once_allowed_in_project_members(settings, seeder, daemon_world, machine_flow):
    # D1: in project_members a non-creator Member may use pod_once.
    # Rust must proceed past the auth (any non-not_issue_runner outcome);
    # Django (no gate) also proceeds.
    # Dedicated-runner issue: pin the dedicated runner onto the issue.
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    _set_policy(seeder, daemon_world, "project_members")
    seeder.db.execute(
        "UPDATE issues SET assigned_runner_id = %s, assigned_pod_id = %s WHERE id = %s",
        (machine_flow["runner_id"], daemon_world["pod"]["id"], issue["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello pod",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
                "pin": "pod_once",
            },
        )
        # Allowed: must not be the policy refusal or pod_once refusal.
        assert resp.status_code != 403 or (
            resp.json().get("reason") not in (REASON, "pod_once_not_allowed")
            and resp.json().get("code") != "pod_once_not_allowed"
        ), resp.text[:500]
        if is_rust():
            body = resp.json()
            assert body.get("reason") != REASON
            assert body.get("code") != "pod_once_not_allowed"
    finally:
        ctx.__exit__(None, None, None)
        seeder.db.execute(
            "UPDATE issues SET assigned_runner_id = NULL, assigned_pod_id = NULL WHERE id = %s",
            (issue["id"],),
        )


def test_pod_once_denied_in_default_mode(settings, seeder, daemon_world, machine_flow):
    # Default mode keeps the old three-owner rule: a non-creator
    # Member is refused with pod_once_not_allowed (Rust) before the
    # run-policy gate (which would also refuse with not_issue_runner).
    issue = _ticking_issue(seeder, daemon_world)
    _set_creator(seeder, issue["id"], daemon_world["owner"]["id"])
    seeder.db.execute(
        "UPDATE issues SET assigned_runner_id = %s, assigned_pod_id = %s WHERE id = %s",
        (machine_flow["runner_id"], daemon_world["pod"]["id"], issue["id"]),
    )
    member = _member(seeder, daemon_world, role=15)
    ctx, client = _member_client(settings, member)
    try:
        resp = web_post(
            client,
            f"{WEB}/runs/",
            {
                "prompt": "hello pod",
                "workspace": daemon_world["workspace"]["id"],
                "work_item": issue["id"],
                "pin": "pod_once",
            },
        )
        assert resp.status_code == expect("DIFF-53", django=201, rust=403), (
            resp.text[:500]
        )
        if is_rust():
            body = resp.json()
            # Either the policy gate or the pod_once gate refuses; both
            # mean no run. The direct tail gates on policy first, so
            # not_issue_runner wins when the caller is not creator/
            # assignee. Pin the expectation loosely: one of the two.
            assert body.get("reason") in (REASON, "pod_once_not_allowed") or body.get(
                "code"
            ) in (REASON, "pod_once_not_allowed")
    finally:
        ctx.__exit__(None, None, None)
        seeder.db.execute(
            "UPDATE issues SET assigned_runner_id = NULL, assigned_pod_id = NULL WHERE id = %s",
            (issue["id"],),
        )
