"""Web runs endpoints (``/api/runners/runs/``, ``re-tick/``).

Paginated "my runs" list, detail (+ ``include_events``), cancel (local
finalize vs cancel-requested fan-out), release-pin escape hatch, and the
re-tick budget grant. POST create paths need dispatch infra, so creation is
pinned through its validation errors.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.web import web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

RUN_KEYS = {
    "id", "status", "executor_kind", "dispatch_attempts", "cancel_requested_at",
    "cancel_reason", "prompt", "thread_id", "agent_metadata", "runner",
    "work_item", "pod", "pod_detail", "created_by", "owner", "created_at",
    "assigned_at", "queue_position", "started_at", "ended_at", "done_payload",
    "error", "error_code", "error_diagnostic", "refusal_category", "llm_model",
    "input_tokens", "output_tokens", "total_tokens", "usage", "tool_plan",
    "tool_calls",
}


def test_web_runs_list_shape(user_client, daemon_run):
    response = user_client.get(f"{WEB}/runs/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert set(body) == {"results", "count", "total_count", "total_pages", "page", "per_page"}
    assert body["total_count"] == 1 and body["count"] == 1
    assert body["page"] == 1 and body["per_page"] == 30 and body["total_pages"] == 1
    run = body["results"][0]
    assert RUN_KEYS <= set(run)
    assert run["id"] == daemon_run["id"]
    assert run["status"] == "assigned"
    assert run["pod_detail"]["project_identifier"] is not None


def test_web_runs_list_denies_anonymous(anon_client):
    response = anon_client.get(f"{WEB}/runs/")
    assert response.status_code == 401
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
    body = user_client.get(f"{WEB}/runs/").json()
    assert foreign["id"] not in [r["id"] for r in body["results"]]
    detail = user_client.get(f"{WEB}/runs/{foreign['id']}/")
    # Cross-workspace runs read as 404 — existence must not leak.
    assert detail.status_code == 404


def test_web_run_detail_shape(user_client, daemon_run):
    response = user_client.get(f"{WEB}/runs/{daemon_run['id']}/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert RUN_KEYS <= set(body)
    assert body["id"] == daemon_run["id"]
    assert "events" not in body


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
    events = response.json()["events"]
    assert [(e["seq"], e["kind"]) for e in events] == [(1, "log")]
    assert set(events[0]) == {"id", "seq", "kind", "payload", "created_at"}


def test_web_run_detail_unknown_is_404(user_client):
    response = user_client.get(f"{WEB}/runs/00000000-0000-0000-0000-000000000000/")
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_web_runs_create_requires_prompt(user_client):
    response = web_post(user_client, f"{WEB}/runs/", {})
    assert response.status_code == 400
    assert response.json() == {"error": "prompt is required"}


def test_web_runs_create_rejects_malformed_work_item(user_client, daemon_world):
    """Malformed work_item UUID escapes validation → unhandled 500 (Django bug)."""
    response = web_post(
        user_client, f"{WEB}/runs/",
        {"prompt": "hi", "workspace": daemon_world["workspace"]["id"], "work_item": "nope"},
    )
    assert response.status_code == 500


def test_web_retick_validates_input(user_client):
    missing = web_post(user_client, f"{WEB}/re-tick/", {})
    assert missing.status_code == 400
    assert missing.json() == {"error": "work_item is required for re-tick"}
    # Re-tick guards the UUID parse (the create path above does not).
    malformed = web_post(user_client, f"{WEB}/re-tick/", {"work_item": "nope"})
    assert malformed.status_code == 400
    assert malformed.json() == {"error": "invalid work_item UUID format"}
    unknown = web_post(
        user_client, f"{WEB}/re-tick/", {"work_item": str(uuid.uuid4())}
    )
    assert unknown.status_code == 404
    assert unknown.json() == {"error": "issue not found"}


def test_web_run_cancel_shape(user_client, daemon_run, seeder):
    response = web_post(user_client, f"{WEB}/runs/{daemon_run['id']}/cancel/", {})
    assert response.status_code == 200, response.text
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
    assert response.json() == {"error": "run already terminal", "code": "run_already_terminal"}


def test_web_run_cancel_unknown_is_404(user_client):
    response = web_post(
        user_client, f"{WEB}/runs/00000000-0000-0000-0000-000000000000/cancel/", {}
    )
    assert response.status_code == 404


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
    assert (
        seeder.db.fetchval(
            "SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run["id"],)
        )
        is None
    )


def test_web_run_release_pin_rejects_unpinned(user_client, daemon_run):
    response = web_post(user_client, f"{WEB}/runs/{daemon_run['id']}/release-pin/", {})
    assert response.status_code == 409
    assert response.json() == {"error": "run not queued"}
