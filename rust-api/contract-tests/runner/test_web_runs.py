"""Web runs endpoints (``/api/runners/runs/``, ``re-tick/``).

Paginated "my runs" list, detail (+ ``include_events``), cancel (local
finalize vs cancel-requested fan-out), release-pin escape hatch, and the
re-tick budget grant. POST create covers all three dispatch tails
(direct, ``run_ai``, ``comment_and_run``): 201 shapes, validation arms,
queue/rollback behavior, and Django's filter-coercion edges.
"""

from __future__ import annotations

import contextlib
import os
import uuid

import pytest

from _harness.auth import login_session
from _harness.http import api_client, api_key_client, bearer_client
from _harness.web import web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

CLOUD_ENABLED = os.environ.get("CONTRACT_CLOUD_AGENT_ENABLED", "false").lower() in (
    "1",
    "true",
    "yes",
)

RUN_KEYS = {
    "id", "status", "executor_kind", "dispatch_attempts", "cancel_requested_at",
    "cancel_reason", "prompt", "thread_id", "agent_metadata", "runner",
    "work_item", "scheduler_binding", "scheduler_binding_detail",
    "pod", "pod_detail", "created_by", "owner", "created_at",
    "assigned_at", "started_at", "ended_at", "done_payload",
    "error", "error_code", "error_diagnostic", "refusal_category", "llm_model",
    "input_tokens", "output_tokens", "total_tokens", "usage", "tool_plan",
    "tool_calls",
}


@pytest.fixture(autouse=True)
def _delete_created_runs(seeder):
    """Remove every run in the test's workspaces afterwards.

    POST create covers all three dispatch tails, so most tests in this
    file leave server-created runs the seeder never tracked; the global
    metrics gauges count them, breaking reused databases. Dependents go
    first (the FKs are checked per statement under autocommit): child
    rows, the binding/ticker run pointers, then children, then the rest.
    """
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


def test_web_runs_list_shape(user_client, daemon_run):
    response = user_client.get(f"{WEB}/runs/")
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert set(body) == {"results", "count", "total_count", "total_pages", "page", "per_page"}
    assert body["total_count"] == 1 and body["count"] == 1
    assert body["page"] == 1 and body["per_page"] == 30 and body["total_pages"] == 1
    run = body["results"][0]
    assert RUN_KEYS <= set(run)
    assert run["id"] == daemon_run["id"]
    assert run["status"] == "assigned"
    assert run["pod_detail"]["project_identifier"] is not None
    assert run["scheduler_binding"] is None
    assert run["scheduler_binding_detail"] is None


def test_web_runs_list_denies_anonymous(anon_client):
    response = anon_client.get(f"{WEB}/runs/")
    assert response.status_code == 401
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_runs_list_hides_foreign_runs(user_client, seeder):
    """Another owner's run never appears in "my runs"."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_project = seeder.create_project(other_ws["id"])
    other_pod = seeder.create_pod(other_ws["id"], other_project["id"], is_default=True)
    other_runner = seeder.enroll_runner(
        other_owner["id"], other_ws["id"], other_pod["id"], f"apd_en_{seeder.tag}fr",
    )
    foreign = seeder.create_daemon_run(
        other_owner["id"], other_ws["id"], other_pod["id"], other_runner["id"]
    )
    listed = user_client.get(f"{WEB}/runs/")
    assert listed.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = listed.json()
    assert foreign["id"] not in [r["id"] for r in body["results"]]
    detail = user_client.get(f"{WEB}/runs/{foreign['id']}/")
    # Cross-workspace runs read as 404 — existence must not leak.
    assert detail.status_code == 404
    assert detail.headers["allow"] == "GET, HEAD, OPTIONS"


def test_web_run_detail_shape(user_client, daemon_run):
    response = user_client.get(f"{WEB}/runs/{daemon_run['id']}/")
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["id"] == daemon_run["id"]
    assert "events" not in body
    assert body["scheduler_binding"] is None
    assert body["scheduler_binding_detail"] is None


def test_web_run_detail_include_events(user_client, machine_client, daemon_run, seeder):
    posted = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/events/",
        json={"events": [{"seq": 1, "kind": "log", "payload": {"text": "hi"}}]},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert posted.json() == {"ok": True, "accepted": 1}
    response = user_client.get(
        f"{WEB}/runs/{daemon_run['id']}/", params={"include_events": "1"}
    )
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    events = response.json()["events"]
    assert [(e["seq"], e["kind"]) for e in events] == [(1, "log")]
    assert set(events[0]) == {"id", "seq", "kind", "payload", "created_at"}


def test_web_run_detail_unknown_is_404(user_client):
    response = user_client.get(f"{WEB}/runs/00000000-0000-0000-0000-000000000000/")
    assert response.status_code == 404
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    assert response.json() == {"error": "not found"}


def test_web_run_detail_null_issue_creator_is_200(user_client, seeder, daemon_world, machine_flow):
    """A run bound to an issue with NULL created_by still reads 200 (PIDASHCONV-1049).

    ``issues.created_by_id`` is nullable; Django dereferences
    ``run.work_item.created_by_id`` → None and continues to the remaining
    grants. The NULL column must not read as a missed join (500).
    """
    issue = seeder.create_issue(daemon_world["workspace"]["id"], daemon_world["project"]["id"])
    assert (
        seeder.db.fetchval("SELECT created_by_id FROM issues WHERE id = %s", (issue["id"],))
        is None
    )
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"], machine_flow["runner_id"],
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s WHERE id = %s", (issue["id"], run["id"])
    )
    response = user_client.get(f"{WEB}/runs/{run['id']}/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["id"] == run["id"]


def test_web_run_detail_null_issue_creator_keeps_assignee_grant(
    settings, seeder, daemon_world, machine_flow
):
    """A live assignee still views the run when the issue creator is NULL (PIDASHCONV-1049).

    Django's work-item arm (``runs.py:107-116``) falls through from the
    ``created_by_id == user`` miss (None never equals) to the live
    through-model assignee check — the NULL creator must not disable it.
    """
    member = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], member["id"])
    issue = seeder.create_issue(daemon_world["workspace"]["id"], daemon_world["project"]["id"])
    assignee_row_id = str(uuid.uuid4())
    seeder.db.execute(
        "INSERT INTO issue_assignees (created_at, updated_at, id, assignee_id,"
        " created_by_id, issue_id, project_id, updated_by_id, workspace_id)"
        " VALUES (now(),now(),%s,%s,%s,%s,%s,%s,%s)",
        (
            assignee_row_id, member["id"], daemon_world["owner"]["id"],
            issue["id"], daemon_world["project"]["id"],
            daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        ),
    )
    seeder.track.add("issue_assignees", assignee_row_id)
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"], machine_flow["runner_id"],
    )
    # Runner-less: the private-runner gate never applies, so the assignee
    # grant is the only path to 200 for this actor (not the run creator,
    # not a workspace admin).
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s, runner_id = NULL WHERE id = %s",
        (issue["id"], run["id"]),
    )
    with api_client(settings.base_url) as client:
        login_session(client, email=member["email"], password=member["password"])
        response = client.get(f"{WEB}/runs/{run['id']}/")
    assert response.status_code == 200, response.text
    assert response.json()["id"] == run["id"]


def test_web_runs_create_requires_prompt(user_client):
    response = web_post(user_client, f"{WEB}/runs/", {})
    assert response.status_code == 400
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {"error": "prompt is required"}


def test_web_runs_create_rejects_malformed_work_item(user_client, daemon_world):
    """Malformed work_item UUID escapes validation → unhandled 500 (Django bug)."""
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": daemon_world["workspace"]["id"], "work_item": "nope"},
    )
    assert response.status_code == 500
    # Raw-500 arm (ported Django bug): no Allow on either backend.
    assert "allow" not in response.headers


def test_web_retick_validates_input(user_client):
    missing = web_post(user_client, f"{WEB}/re-tick/", {})
    assert missing.status_code == 400
    assert missing.headers["allow"] == "POST, OPTIONS"
    assert missing.json() == {"error": "work_item is required for re-tick"}
    # Re-tick guards the UUID parse (the create path above does not).
    malformed = web_post(user_client, f"{WEB}/re-tick/", {"work_item": "nope"})
    assert malformed.status_code == 400
    assert malformed.headers["allow"] == "POST, OPTIONS"
    assert malformed.json() == {"error": "invalid work_item UUID format"}
    unknown = web_post(
        user_client, f"{WEB}/re-tick/", {"work_item": str(uuid.uuid4())}
    )
    assert unknown.status_code == 404
    assert unknown.headers["allow"] == "POST, OPTIONS"
    assert unknown.json() == {"error": "issue not found"}


def test_web_run_cancel_shape(user_client, daemon_run, seeder):
    response = web_post(user_client, f"{WEB}/runs/{daemon_run['id']}/cancel/", {})
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "POST, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    # Assigned local run on an offline runner: cancel-requested, fan-out
    # best-effort (the offline runner picks it up on its next session).
    assert body["status"] == "cancel_requested"
    assert body["cancel_reason"] == "cancelled by user"
    assert (
        seeder.db.fetchval(
            "SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],)
        )
        == "cancel_requested"
    )


def test_web_run_cancel_terminal_is_409(user_client, seeder, daemon_world, machine_flow):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"], machine_flow["runner_id"], status="completed",
    )
    response = web_post(user_client, f"{WEB}/runs/{run['id']}/cancel/", {})
    assert response.status_code == 409
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "run already terminal", "code": "run_already_terminal"}


def test_web_run_cancel_unknown_is_404(user_client):
    response = web_post(
        user_client, f"{WEB}/runs/00000000-0000-0000-0000-000000000000/cancel/", {}
    )
    assert response.status_code == 404
    assert response.headers["allow"] == "POST, OPTIONS"


def test_web_run_release_pin_shape(user_client, seeder, daemon_world, machine_flow):
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"], daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"], machine_flow["runner_id"], status="queued",
    )
    seeder.db.execute(
        "UPDATE agent_run SET pinned_runner_id = %s WHERE id = %s",
        (machine_flow["runner_id"], run["id"]),
    )
    response = web_post(user_client, f"{WEB}/runs/{run['id']}/release-pin/", {})
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "POST, OPTIONS"
    assert (
        seeder.db.fetchval(
            "SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run["id"],)
        )
        is None
    )


def test_web_run_release_pin_rejects_unpinned(user_client, daemon_run):
    response = web_post(user_client, f"{WEB}/runs/{daemon_run['id']}/release-pin/", {})
    assert response.status_code == 409
    assert response.headers["allow"] == "POST, OPTIONS"
    assert response.json() == {"error": "run not queued"}


def _ticking_issue(seeder, daemon_world):
    state = seeder.create_state(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        name="In Progress", group="started",
    )
    return seeder.create_issue(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"], state["id"]
    )


def test_web_runs_create_direct_freeform_201(user_client, seeder, daemon_world):
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hello free", "workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["status"] == "queued"
    assert body["executor_kind"] == "local_runner"
    assert body["prompt"] == "hello free"
    assert body["work_item"] is None
    assert body["pod"] == daemon_world["pod"]["id"]
    assert body["created_by"] == daemon_world["owner"]["id"]
    assert body["runner"] is None and body["owner"] is None
    assert body["tool_calls"] == []
    row = seeder.db.fetchone(
        "SELECT trigger, phase_kind, run_config, required_capabilities,"
        " prompt_manifest, dispatch_attempts FROM agent_run WHERE id = %s",
        (body["id"],),
    )
    assert row["trigger"] == "direct" and row["phase_kind"] == ""
    assert row["run_config"] == {} and row["required_capabilities"] == []
    assert row["prompt_manifest"] is None and row["dispatch_attempts"] == 0


def test_web_runs_create_direct_issue_201(user_client, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    response = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hello issue",
            "workspace": daemon_world["workspace"]["id"],
            "work_item": issue["id"],
            "run_config": {"a": 1},
            "required_capabilities": ["x"],
        },
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["work_item"] == issue["id"]
    assert body["prompt"] == "hello issue"
    row = seeder.db.fetchone(
        "SELECT run_config, required_capabilities FROM agent_run WHERE id = %s",
        (body["id"],),
    )
    assert row["run_config"] == {"a": 1} and row["required_capabilities"] == ["x"]
    # The active-run guardrail: a second direct dispatch on the issue 409s.
    again = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "second",
            "workspace": daemon_world["workspace"]["id"],
            "work_item": issue["id"],
        },
    )
    assert again.status_code == 409
    assert again.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert again.json() == {
        "error": "work item already has an active run", "code": "active_run_exists",
    }


def test_web_runs_create_direct_cloud_not_configured_409(user_client, seeder, daemon_world):
    """Cloud-requested direct create with the instance switch off (PIDASHCONV-1051).

    Contract CI runs both backends with CLOUD_AGENT_ENABLED off, so the
    executor-resolution arm — not the creator llm gate — answers here. This
    pins the exact pre-gate 409 bytes around the seam consult: the llm gate
    itself is pinned by the Rust seam tests (CE default answers the key bit,
    an installed seam wins), since no HTTP path reaches it flags-off.
    """
    seeder.db.execute(
        "UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = %s",
        (daemon_world["project"]["id"],),
    )
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hello cloud", "workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 409, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {
        "error": "Pi Dash Cloud Agent is not currently available",
        "code": "cloud_agent_unavailable",
    }


def test_web_runs_create_direct_validation(user_client, seeder, daemon_world):
    ws = daemon_world["workspace"]["id"]
    missing_ws = web_post(user_client, f"{WEB}/runs/", {"prompt": "hi"})
    assert missing_ws.status_code == 400
    assert missing_ws.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert missing_ws.json() == {"error": "workspace is required", "code": "workspace_required"}
    # Another workspace the caller never joined.
    outsider = seeder.create_user()
    foreign_ws = seeder.create_workspace(outsider["id"])
    foreign = web_post(user_client, f"{WEB}/runs/", {"prompt": "hi", "workspace": foreign_ws["id"]})
    assert foreign.status_code == 403
    assert foreign.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert foreign.json() == {
        "error": "caller is not a member of the target workspace",
        "code": "not_workspace_member",
    }
    fake_issue = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": ws, "work_item": str(uuid.uuid4())},
    )
    assert fake_issue.status_code == 400
    assert fake_issue.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert fake_issue.json() == {"error": "work_item does not exist", "code": "work_item_missing"}
    fake_pod = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": ws, "pod": str(uuid.uuid4())},
    )
    assert fake_pod.status_code == 400
    assert fake_pod.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert fake_pod.json() == {
        "error": "pod does not exist or has been deleted", "code": "pod_missing",
    }
    # No pod anywhere: a second project without a default pod disables the
    # single-project back-compat leg.
    seeder.create_project(ws)
    nopod = web_post(user_client, f"{WEB}/runs/", {"prompt": "hi", "workspace": ws})
    assert nopod.status_code == 409
    assert nopod.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert nopod.json() == {
        "error": "no pod available; ensure the issue has a project with a default pod",
        "code": "no_pod_available",
    }


def test_web_runs_create_direct_coercions(user_client, seeder, daemon_world):
    """Django `filter()` coercion: ints ride `UUID(int=)`, garbage 500s."""
    ws = daemon_world["workspace"]["id"]
    issue = _ticking_issue(seeder, daemon_world)
    ws_int = web_post(user_client, f"{WEB}/runs/", {"prompt": "hi", "workspace": 123})
    assert ws_int.status_code == 403
    assert ws_int.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert ws_int.json()["code"] == "not_workspace_member"
    wi_int = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": ws, "work_item": 123},
    )
    assert wi_int.status_code == 400
    assert wi_int.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert wi_int.json()["code"] == "work_item_missing"
    pod_int = web_post(
        user_client, f"{WEB}/runs/", {"prompt": "hi", "workspace": ws, "pod": 123}
    )
    assert pod_int.status_code == 400
    assert pod_int.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert pod_int.json()["code"] == "pod_missing"
    for bad in ("nope", 1.5, [1], {"a": 1}):
        response = web_post(
            user_client, f"{WEB}/runs/",
            {"prompt": "hi", "workspace": ws, "work_item": bad},
        )
        assert response.status_code == 500, bad
        assert "allow" not in response.headers, bad
    triggered = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": ws, "triggered_by": 123},
    )
    assert triggered.status_code == 500
    assert "allow" not in triggered.headers
    # Non-canonical workspace text passes membership (the filter coerces)
    # but fails the `str() != str()` consistency comparisons.
    upper = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": ws.upper(), "work_item": issue["id"]},
    )
    assert upper.status_code == 400
    assert upper.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert upper.json()["code"] == "work_item_workspace_mismatch"
    # Capabilities/config store verbatim on the local path, uninspected.
    stored = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi", "workspace": ws,
            "required_capabilities": "abc", "run_config": 5,
        },
    )
    assert stored.status_code == 201, stored.text
    assert stored.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    row = seeder.db.fetchone(
        "SELECT required_capabilities, run_config FROM agent_run WHERE id = %s",
        (stored.json()["id"],),
    )
    assert row["required_capabilities"] == "abc" and row["run_config"] == 5


def test_web_runs_create_prompt_str_coercion(user_client, seeder, daemon_world):
    """Non-string prompts save as Python `str()` (probed)."""
    ws = daemon_world["workspace"]["id"]
    for prompt, saved in [
        (5, "5"), (True, "True"), (1.5, "1.5"), (1e16, "1e+16"),
        ([1, 2], "[1, 2]"), ({"a": 1}, "{'a': 1}"),
    ]:
        response = web_post(user_client, f"{WEB}/runs/", {"prompt": prompt, "workspace": ws})
        assert response.status_code == 201, prompt
        assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS", prompt
        assert response.json()["prompt"] == saved, prompt


def test_web_runs_create_runai_validation(user_client, seeder, daemon_world):
    missing = web_post(user_client, f"{WEB}/runs/", {"triggered_by": "run_ai"})
    assert missing.status_code == 400
    assert missing.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert missing.json() == {"error": "work_item is required for run_ai"}
    fake = web_post(
        user_client, f"{WEB}/runs/",
        {"triggered_by": "run_ai", "work_item": str(uuid.uuid4())},
    )
    assert fake.status_code == 404
    assert fake.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert fake.json() == {"error": "issue not found"}
    # Ints coerce via `UUID(int=)` and miss — a 404, not a 500.
    coerce = web_post(user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": 123})
    assert coerce.status_code == 404
    assert coerce.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    outsider = seeder.create_user()
    foreign_ws = seeder.create_workspace(outsider["id"])
    foreign_project = seeder.create_project(foreign_ws["id"])
    foreign_state = seeder.create_state(foreign_ws["id"], foreign_project["id"])
    foreign_issue = seeder.create_issue(foreign_ws["id"], foreign_project["id"], foreign_state["id"])
    walled = web_post(
        user_client, f"{WEB}/runs/",
        {"triggered_by": "run_ai", "work_item": foreign_issue["id"]},
    )
    assert walled.status_code == 404
    assert walled.headers["allow"] == "GET, POST, HEAD, OPTIONS"


def test_web_runs_create_runai_no_runner_409_rolls_back(user_client, seeder, daemon_world):
    """Without a runner the preflight bounces — and the web atomic rolls
    the bounce (state move, comment, re-time) back: the 409 is traceless."""
    issue = _ticking_issue(seeder, daemon_world)
    before = seeder.db.fetchone(
        "SELECT state_id FROM issues WHERE id = %s", (issue["id"],)
    )
    response = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert response.status_code == 409
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {
        "error": "could not dispatch — issue may already have an active run, or no pod is available"
    }
    after = seeder.db.fetchone("SELECT state_id FROM issues WHERE id = %s", (issue["id"],))
    assert after["state_id"] == before["state_id"]
    assert seeder.db.fetchone(
        "SELECT id FROM issue_comments WHERE issue_id = %s", (issue["id"],)
    ) is None
    assert seeder.db.fetchone(
        "SELECT id FROM issue_agent_ticker WHERE issue_id = %s", (issue["id"],)
    ) is None
    assert seeder.db.fetchone(
        "SELECT id FROM agent_run WHERE work_item_id = %s", (issue["id"],)
    ) is None


def test_web_runs_create_runai_201(user_client, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    response = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["work_item"] == issue["id"]
    assert body["status"] == "queued" and body["executor_kind"] == "local_runner"
    assert body["prompt"]  # the phase template rendered server-side
    row = seeder.db.fetchone(
        "SELECT trigger, prompt_manifest FROM agent_run WHERE id = %s", (body["id"],)
    )
    assert row["trigger"] == "run_ai" and row["prompt_manifest"] is not None


def _open_and_poll_runner_session(client, runner_id):
    """Open a runner session and poll once so the runner is ONLINE + idle.

    Session open alone leaves the runner `offline`; the poll flips it
    `ONLINE` (`runner/views/sessions.py`) and refreshes the heartbeat,
    which is what makes the matcher consider the runner for new work.
    """
    opened = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert opened.status_code == 201, opened.text
    poll_url = (
        f"{DAEMON}/runners/{runner_id}/sessions/{opened.json()['session_id']}/poll"
    )
    payload = {"ack": [], "status": {"status": "idle"}}
    polled = client.post(poll_url, json=payload)
    for _ in range(2):
        # Django's shared async-Redis client can 500 a poll under the dev
        # server (`Event loop is closed`); retry like test_daemon_sessions.
        if polled.status_code != 500:
            break
        polled = client.post(poll_url, json=payload)
    assert polled.status_code == 200, polled.text


@pytest.fixture()
def online_runner_pair(settings, seeder, daemon_world, machine_flow, machine_client):
    """Two online idle runners in the default pod (PIDASHCONV-1481).

    `machine_flow`'s runner plus a second enrolled runner, each with a live
    session. The matcher only drains to ONLINE, heartbeat-fresh, idle
    runners, so each POST below finds a free runner.
    """
    _open_and_poll_runner_session(machine_client, machine_flow["runner_id"])
    host_label = f"online-{seeder.tag}b"[:200]
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        created = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host_label,
            },
        )
    assert created.status_code == 201, created.text
    second_id = created.json()["runner_id"]
    with bearer_client(
        settings.base_url, created.json()["machine_token"],
        headers={"X-Runner-Id": second_id},
    ) as client:
        _open_and_poll_runner_session(client, second_id)
    yield [machine_flow["runner_id"], second_id]
    # Teardown mirrors machine_flow's finalizer (conftest.py) for the
    # endpoint-created rows the seeder never tracked.
    owner_id = daemon_world["owner"]["id"]
    seeder.db.execute("DELETE FROM runner_session WHERE runner_id = %s", (second_id,))
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = NULL "
        "WHERE runner_id = %s OR pinned_runner_id = %s",
        (second_id, second_id),
    )
    seeder.db.execute("DELETE FROM agent_chat_session WHERE runner_id = %s", (second_id,))
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (owner_id, host_label),
    )
    seeder.db.execute("DELETE FROM runner WHERE id = %s", (second_id,))
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (owner_id, host_label),
    )


def test_web_runs_create_drains_pod_to_idle_runner(
    user_client, seeder, daemon_world, online_runner_pair
):
    """run_ai + direct POSTs assign in-request with an idle online runner.

    Django drains the pod in `on_commit(dispatch_agent_run)`, so each POST
    assigns to an idle runner before the 201 (PIDASHCONV-1481). The row —
    not the 201 body, which renders stale-queued for run_ai — pins this.
    """
    issue = _ticking_issue(seeder, daemon_world)
    runai = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert runai.status_code == 201, runai.text
    assert runai.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    first = seeder.db.fetchone(
        "SELECT status, runner_id, owner_id, assigned_at FROM agent_run WHERE id = %s",
        (runai.json()["id"],),
    )
    assert first["status"] == "assigned"
    assert str(first["runner_id"]) in online_runner_pair
    assert first["owner_id"] is not None and first["assigned_at"] is not None
    direct = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hello assigned", "workspace": daemon_world["workspace"]["id"]},
    )
    assert direct.status_code == 201, direct.text
    assert direct.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    second = seeder.db.fetchone(
        "SELECT status, runner_id, owner_id, assigned_at FROM agent_run WHERE id = %s",
        (direct.json()["id"],),
    )
    assert second["status"] == "assigned"
    assert str(second["runner_id"]) in online_runner_pair
    assert str(second["runner_id"]) != str(first["runner_id"])
    assert second["owner_id"] is not None and second["assigned_at"] is not None


def test_web_runs_create_runai_201_body_is_predispatch_with_idle_runner(
    user_client, seeder, daemon_world, online_runner_pair
):
    """run_ai 201 renders the pre-dispatch run while the row assigns (PIDASHCONV-1482).

    Django serializes the in-memory run with no ``refresh_from_db``
    (``runs.py`` ``_post_run_ai``), so the body stays queued and runner-less
    even though the on-commit dispatch assigned the row in-request.
    """
    issue = _ticking_issue(seeder, daemon_world)
    response = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["executor_kind"] == "local_runner"
    assert body["status"] == "queued"
    assert body["runner"] is None
    assert body["owner"] is None
    assert body["assigned_at"] is None
    assert body["dispatch_attempts"] == 0
    row = seeder.db.fetchone(
        "SELECT status, runner_id, owner_id, assigned_at FROM agent_run WHERE id = %s",
        (body["id"],),
    )
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) in online_runner_pair
    assert row["owner_id"] is not None and row["assigned_at"] is not None


def _seed_llm_config(seeder, user_id):
    """BYOK marker row so ``user_has_llm_config`` passes (cf. dispatch/conftest.py)."""
    seeder.db.execute(
        """INSERT INTO assistant_user_llm_config
           (provider_kind, base_url, model_name, api_key_encrypted,
            last_verified_at, created_at, updated_at, user_id)
           VALUES ('openai_compatible','https://llm.example.invalid/v1','contract-model',
                   decode('deadbeef','hex'),NULL,now(),now(),%s)""",
        (user_id,),
    )
    row_id = seeder.db.fetchval(
        "SELECT id FROM assistant_user_llm_config WHERE user_id = %s", (user_id,)
    )
    seeder.track.add("assistant_user_llm_config", row_id)


def test_web_runs_create_runai_201_body_is_predispatch_for_cloud(
    user_client, seeder, daemon_world
):
    """Cloud run_ai 201 keeps dispatch_attempts 0 while the row leases to 1 (PIDASHCONV-1482).

    Regime: servers booted with ``CLOUD_AGENT_ENABLED=true``
    (``CONTRACT_CLOUD_AGENT_ENABLED=true``); skipped otherwise — CI runs the
    kill-switches off, where a cloud-default project refuses the create.
    """
    if not CLOUD_ENABLED:
        pytest.skip("needs CLOUD_AGENT_ENABLED=true servers (CONTRACT_CLOUD_AGENT_ENABLED)")
    seeder.db.execute(
        "UPDATE projects SET default_agent_executor = 'cloud_agent' WHERE id = %s",
        (daemon_world["project"]["id"],),
    )
    _seed_llm_config(seeder, daemon_world["owner"]["id"])
    # The cloud creator must hold a project role (check_project_role in
    # _resolve_creator_for_trigger); the local path returns the actor bare.
    seeder.create_project_member(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        daemon_world["owner"]["id"], role=15,
    )
    issue = _ticking_issue(seeder, daemon_world)
    response = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["executor_kind"] == "cloud_agent"
    assert body["status"] == "queued"
    assert body["dispatch_attempts"] == 0
    row = seeder.db.fetchone(
        "SELECT status, dispatch_attempts, lease_expires_at FROM agent_run WHERE id = %s",
        (body["id"],),
    )
    assert row["status"] == "queued"
    assert row["dispatch_attempts"] == 1
    assert row["lease_expires_at"] is not None


def test_web_runs_create_runai_queued_202(user_client, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    seeder.create_agent_run(
        daemon_world["workspace"]["id"], daemon_world["owner"]["id"],
        daemon_world["pod"]["id"], issue["id"], status="running",
    )
    response = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert response.status_code == 202
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {
        "queued": True,
        "reason": "free-entry-queued",
        "detail": "a run is active on this work item; the next run is queued and starts when it ends",
    }
    ticker = seeder.db.fetchone(
        "SELECT pending_entry, pending_entry_free, pending_entry_actor_id,"
        " pending_entry_trigger FROM issue_agent_ticker WHERE issue_id = %s",
        (issue["id"],),
    )
    assert ticker["pending_entry"] is True and ticker["pending_entry_free"] is True
    assert str(ticker["pending_entry_actor_id"]) == daemon_world["owner"]["id"]
    assert ticker["pending_entry_trigger"] == "run_ai"


def test_web_runs_create_comment_run_no_prior_409(user_client, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"triggered_by": "comment_and_run", "work_item": issue["id"]},
    )
    assert response.status_code == 409
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {
        "error": "could not dispatch — issue may already have an active run,"
        " or no prior run / pod is available"
    }


def test_web_runs_create_comment_run_201(user_client, seeder, daemon_world, machine_flow):
    issue = _ticking_issue(seeder, daemon_world)
    prior = seeder.create_agent_run(
        daemon_world["workspace"]["id"], daemon_world["owner"]["id"],
        daemon_world["pod"]["id"], issue["id"], status="completed",
    )
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"triggered_by": "comment_and_run", "work_item": issue["id"]},
    )
    assert response.status_code == 201, response.text
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["work_item"] == issue["id"]
    assert body["status"] == "queued"
    row = seeder.db.fetchone(
        "SELECT trigger, parent_run_id FROM agent_run WHERE id = %s", (body["id"],)
    )
    assert row["trigger"] == "comment_and_run"
    # A fresh session off the terminal prior: no parent pin.
    assert row["parent_run_id"] is None


def test_web_runs_create_comment_run_queued_202(user_client, seeder, daemon_world):
    issue = _ticking_issue(seeder, daemon_world)
    seeder.create_agent_run(
        daemon_world["workspace"]["id"], daemon_world["owner"]["id"],
        daemon_world["pod"]["id"], issue["id"], status="running",
    )
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"triggered_by": "comment_and_run", "work_item": issue["id"]},
    )
    assert response.status_code == 202
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json()["reason"] == "free-entry-queued"
    ticker = seeder.db.fetchone(
        "SELECT pending_entry_trigger FROM issue_agent_ticker WHERE issue_id = %s",
        (issue["id"],),
    )
    assert ticker["pending_entry_trigger"] == "comment_and_run"


def test_web_runs_create_soft_deleted_issue(user_client, seeder, daemon_world):
    """Direct reads the default manager (400); `run_ai` reads
    `all_objects` and proceeds."""
    issue = _ticking_issue(seeder, daemon_world)
    seeder.create_agent_run(
        daemon_world["workspace"]["id"], daemon_world["owner"]["id"],
        daemon_world["pod"]["id"], issue["id"], status="running",
    )
    seeder.db.execute("UPDATE issues SET deleted_at = now() WHERE id = %s", (issue["id"],))
    direct = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi",
            "workspace": daemon_world["workspace"]["id"],
            "work_item": issue["id"],
        },
    )
    assert direct.status_code == 400
    assert direct.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert direct.json()["code"] == "work_item_missing"
    runai = web_post(
        user_client, f"{WEB}/runs/", {"triggered_by": "run_ai", "work_item": issue["id"]}
    )
    assert runai.status_code == 202
    assert runai.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert runai.json()["reason"] == "free-entry-queued"


def test_web_runs_create_direct_coercion_mismatch_order(user_client, seeder, daemon_world):
    """Django's work-item `str()` check fires before pod resolution."""
    ws = daemon_world["workspace"]["id"]
    issue = _ticking_issue(seeder, daemon_world)
    upper = ws.upper()
    # Missing pod + non-canonical workspace: the work-item check wins.
    missing = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi", "workspace": upper, "work_item": issue["id"],
            "pod": str(uuid.uuid4()),
        },
    )
    assert missing.status_code == 400
    assert missing.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert missing.json()["code"] == "work_item_workspace_mismatch"
    # Foreign pod (exists, another workspace): same — work-item check first.
    outsider = seeder.create_user()
    foreign_ws = seeder.create_workspace(outsider["id"])
    foreign_project = seeder.create_project(foreign_ws["id"])
    foreign_pod = seeder.create_pod(foreign_ws["id"], foreign_project["id"])
    foreign = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi", "workspace": upper, "work_item": issue["id"],
            "pod": foreign_pod["id"],
        },
    )
    assert foreign.status_code == 400
    assert foreign.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert foreign.json()["code"] == "work_item_workspace_mismatch"
    # Control: canonical text keeps the pod errors.
    canon_missing = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi", "workspace": ws, "work_item": issue["id"],
            "pod": str(uuid.uuid4()),
        },
    )
    assert canon_missing.status_code == 400
    assert canon_missing.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert canon_missing.json()["code"] == "pod_missing"
    canon_foreign = web_post(
        user_client, f"{WEB}/runs/",
        {
            "prompt": "hi", "workspace": ws, "work_item": issue["id"],
            "pod": foreign_pod["id"],
        },
    )
    assert canon_foreign.status_code == 400
    assert canon_foreign.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert canon_foreign.json()["code"] == "pod_workspace_mismatch"


# ---------------------------------------------------------------------------
# Scheduler-fired runs (PDASHOSS01-223, PIDASHCONV-1060)
# ---------------------------------------------------------------------------


def _seed_scheduler(seeder, workspace_id, slug):
    """One scheduler definition. Column list mirrors the legacy harness
    ``Seed.scheduler`` (``db/models/scheduler.py:107``)."""
    sched_id = str(uuid.uuid4())
    name = f"CT1060 {slug}"
    seeder.db.execute(
        """INSERT INTO schedulers
           (id, workspace_id, slug, name, description, prompt, source,
            is_enabled, color, created_at, updated_at)
           VALUES (%s,%s,%s,%s,'contract scheduler','Audit the project.',
                   'builtin',true,'#3b82f6',now(),now())""",
        (sched_id, workspace_id, slug, name),
    )
    seeder.track.add("schedulers", sched_id)
    return {"id": sched_id, "slug": slug, "name": name}


def _seed_binding(seeder, workspace_id, project_id, scheduler_id, actor_id):
    """One install of a scheduler onto a project (``db/models/scheduler.py:154``)."""
    binding_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO scheduler_bindings
           (id, workspace_id, project_id, scheduler_id, actor_id, dtstart,
            tzid, rrule, rdates, exdates, extra_context, enabled, last_error,
            outcome_mode, created_at, updated_at)
           VALUES (%s,%s,%s,%s,%s,now(),
                   'UTC','','[]','[]','',true,'',
                   'create_issue',now(),now())""",
        (binding_id, workspace_id, project_id, scheduler_id, actor_id),
    )
    seeder.track.add("scheduler_bindings", binding_id)
    return {"id": binding_id}


def _seed_scheduler_run(seeder, daemon_world, binding_id, *, status="completed"):
    """A runner-less, issue-less run fired by a scheduler binding.

    Mirrors Django's ``_make_run`` (``test_binding_runs.py:102``): workspace,
    creator, pod and ``scheduler_binding`` set, no runner, no work item.
    """
    run_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, workspace_id, created_by_id, pod_id, scheduler_binding_id,
            status, prompt, trigger, executor_kind, run_config,
            required_capabilities, thread_id, agent_metadata, tool_plan,
            usage, dispatch_attempts, error, refusal_category, llm_model,
            cancel_reason, error_code, phase_kind, created_at)
           VALUES (%s,%s,%s,%s,%s,
                   %s,'','scheduler','local_runner','{}',
                   '[]','{}','{}','{}',
                   '{}',0,'','','',
                   '','','',now())""",
        (
            run_id, daemon_world["workspace"]["id"], daemon_world["owner"]["id"],
            daemon_world["pod"]["id"], binding_id, status,
        ),
    )
    seeder.track.add("agent_run", run_id)
    return {"id": run_id}


def _scheduler_world(seeder, daemon_world, slug):
    sched = _seed_scheduler(seeder, daemon_world["workspace"]["id"], f"{slug}-{seeder.tag}")
    binding = _seed_binding(
        seeder, daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        sched["id"], daemon_world["owner"]["id"],
    )
    return {"scheduler": sched, "binding": binding}


@contextlib.contextmanager
def _login_as(settings, user):
    with api_client(settings.base_url) as client:
        login_session(client, email=user["email"], password=user["password"])
        yield client


def test_web_runs_list_scheduler_binding_detail_shape(
    user_client, seeder, daemon_world, daemon_run
):
    """Scheduler-fired rows render ``scheduler_binding_detail`` exactly."""
    world = _scheduler_world(seeder, daemon_world, "nightly-audit")
    run = _seed_scheduler_run(seeder, daemon_world, world["binding"]["id"])
    body = user_client.get(f"{WEB}/runs/").json()
    rows = {r["id"]: r for r in body["results"]}
    assert rows[daemon_run["id"]]["scheduler_binding"] is None
    assert rows[daemon_run["id"]]["scheduler_binding_detail"] is None
    row = rows[run["id"]]
    assert row["scheduler_binding"] == world["binding"]["id"]
    assert row["scheduler_binding_detail"] == {
        "id": world["binding"]["id"],
        "scheduler_name": world["scheduler"]["name"],
        "scheduler_slug": world["scheduler"]["slug"],
        "project": daemon_world["project"]["id"],
    }


def test_web_run_detail_visible_to_project_member_for_scheduler_run(
    settings, seeder, daemon_world
):
    """Port of ``test_run_detail_visible_to_project_member_for_scheduler_run``.

    The member is neither creator, runner owner, assignee nor admin — the
    scheduler-binding project grant (``runs.py:117``) is their only path.
    """
    world = _scheduler_world(seeder, daemon_world, "nightly-audit")
    run = _seed_scheduler_run(seeder, daemon_world, world["binding"]["id"])
    member = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], member["id"])
    seeder.create_project_member(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        member["id"], role=15,
    )
    with _login_as(settings, member) as client:
        response = client.get(f"{WEB}/runs/{run['id']}/")
    assert response.status_code == 200, response.text
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    assert response.json()["scheduler_binding_detail"]["scheduler_slug"] == (
        world["scheduler"]["slug"]
    )


def test_web_run_detail_hidden_from_workspace_member_outside_project(
    settings, seeder, daemon_world
):
    """Port of ``test_run_detail_hidden_from_workspace_member_outside_project``."""
    world = _scheduler_world(seeder, daemon_world, "nightly-audit")
    run = _seed_scheduler_run(seeder, daemon_world, world["binding"]["id"])
    outsider = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], outsider["id"])
    with _login_as(settings, outsider) as client:
        response = client.get(f"{WEB}/runs/{run['id']}/")
    assert response.status_code == 404
    assert response.headers["allow"] == "GET, HEAD, OPTIONS"
    assert response.json() == {"error": "not found"}


def test_web_runs_list_filters_by_scheduler_binding(user_client, seeder, daemon_world):
    """Port of ``test_global_runs_list_filters_by_scheduler_binding``."""
    first = _scheduler_world(seeder, daemon_world, "nightly-audit")
    second = _scheduler_world(seeder, daemon_world, "weekly-report")
    mine = _seed_scheduler_run(seeder, daemon_world, first["binding"]["id"])
    _seed_scheduler_run(seeder, daemon_world, second["binding"]["id"])
    body = user_client.get(
        f"{WEB}/runs/", params={"scheduler_binding": first["binding"]["id"]}
    ).json()
    assert [r["id"] for r in body["results"]] == [mine["id"]]
    assert body["total_count"] == 1
    # An empty value scopes nothing (Django ``if scheduler_binding_id:``).
    unfiltered = user_client.get(f"{WEB}/runs/", params={"scheduler_binding": ""}).json()
    assert unfiltered["total_count"] == 2


def test_web_runs_list_rejects_malformed_scheduler_binding(user_client):
    """Port of ``test_global_runs_list_rejects_malformed_scheduler_binding``."""
    response = user_client.get(f"{WEB}/runs/", params={"scheduler_binding": "not-a-uuid"})
    assert response.status_code == 400
    assert response.headers["allow"] == "GET, POST, HEAD, OPTIONS"
    assert response.json() == {"error": "invalid scheduler_binding UUID format"}


def test_web_run_detail_soft_deleted_binding_still_renders(
    settings, user_client, seeder, daemon_world
):
    """Runs of an uninstalled binding stay visible and attributed.

    The FK resolves through the base manager (no ``deleted_at`` scope), so
    the detail keeps rendering and the project grant keeps applying.
    """
    world = _scheduler_world(seeder, daemon_world, "nightly-audit")
    run = _seed_scheduler_run(seeder, daemon_world, world["binding"]["id"])
    member = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], member["id"])
    seeder.create_project_member(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        member["id"], role=15,
    )
    seeder.db.execute(
        "UPDATE scheduler_bindings SET deleted_at = now() WHERE id = %s",
        (world["binding"]["id"],),
    )
    detail = user_client.get(f"{WEB}/runs/{run['id']}/")
    assert detail.status_code == 200, detail.text
    assert detail.json()["scheduler_binding_detail"] == {
        "id": world["binding"]["id"],
        "scheduler_name": world["scheduler"]["name"],
        "scheduler_slug": world["scheduler"]["slug"],
        "project": daemon_world["project"]["id"],
    }
    with _login_as(settings, member) as client:
        member_view = client.get(f"{WEB}/runs/{run['id']}/")
    assert member_view.status_code == 200, member_view.text
    filtered = user_client.get(
        f"{WEB}/runs/", params={"scheduler_binding": world["binding"]["id"]}
    ).json()
    assert [r["id"] for r in filtered["results"]] == [run["id"]]
