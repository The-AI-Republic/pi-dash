"""Web dev-machine endpoints (``/api/runners/dev-machines/...``).

List (workspace-scoped, annotated counts), revoke (revokes machine + tokens
+ runners), rotate (token invalidation without revoke), cloud-driven
``create-runner`` enqueue + status poll, hard delete. The suite's machines
are idle (no control session), so ``create-runner`` deterministically takes
the ``409 machine_offline`` path.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.web import web_delete, web_post

WEB = "/api/runners"

pytestmark = pytest.mark.contract

MACHINE_KEYS = {
    "id", "host_label", "label", "visibility", "runner_count",
    "online_runner_count", "control_online", "last_seen_at",
    "last_heartbeat_at", "revoked_at", "created_at", "updated_at",
}


def _machine_id(user_client, daemon_world, machine_flow) -> str:
    response = user_client.get(
        f"{WEB}/dev-machines/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    for machine in response.json():
        if machine["host_label"] == machine_flow["host_label"]:
            return machine["id"]
    raise AssertionError(f"flow machine {machine_flow['host_label']} missing from list")


def test_web_dev_machine_list_shape(user_client, daemon_world, machine_flow):
    response = user_client.get(
        f"{WEB}/dev-machines/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    machine = body[0]
    assert MACHINE_KEYS <= set(machine)
    assert machine["host_label"] == machine_flow["host_label"]
    assert machine["runner_count"] == 1
    assert machine["online_runner_count"] == 0
    assert machine["control_online"] is False
    assert machine["revoked_at"] is None


def test_web_dev_machine_list_requires_workspace(user_client):
    response = user_client.get(f"{WEB}/dev-machines/")
    assert response.status_code == 400
    assert response.json() == {"error": "workspace is required"}


def test_web_dev_machine_list_denies_anonymous(anon_client, daemon_world):
    response = anon_client.get(
        f"{WEB}/dev-machines/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_dev_machine_list_isolates_tenants(user_client, seeder, daemon_world):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    forbidden = user_client.get(f"{WEB}/dev-machines/", params={"workspace": other_ws["id"]})
    assert forbidden.status_code == 403
    assert forbidden.json() == {"error": "forbidden"}


def test_web_dev_machine_revoke_shape(user_client, daemon_world, machine_flow, seeder):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/revoke/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert MACHINE_KEYS <= set(body)
    assert body["revoked_at"] is not None
    assert (
        seeder.db.fetchval("SELECT revoked_at FROM dev_machine WHERE id = %s", (machine_id,))
        is not None
    )
    # Revoke cascades to the machine's runner.
    assert (
        seeder.db.fetchval(
            "SELECT revoked_at FROM runner WHERE id = %s", (machine_flow["runner_id"],)
        )
        is not None
    )


def test_web_dev_machine_rotate_shape(user_client, daemon_world, machine_flow, seeder):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/rotate/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert MACHINE_KEYS <= set(body)
    assert body["revoked_at"] is None
    # Rotate invalidates the machine token without revoking the machine.
    assert (
        seeder.db.fetchval(
            "SELECT COUNT(*) FROM machine_token WHERE user_id = %s AND revoked_at IS NULL",
            (daemon_world["owner"]["id"],),
        )
        == 0
    )


def test_web_dev_machine_rotate_after_revoke_is_409(user_client, daemon_world, machine_flow):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    ws = {"workspace": daemon_world["workspace"]["id"]}
    assert web_post(user_client, f"{WEB}/dev-machines/{machine_id}/revoke/", ws).status_code == 200
    response = web_post(user_client, f"{WEB}/dev-machines/{machine_id}/rotate/", ws)
    assert response.status_code == 409
    assert response.json() == {"error": "dev_machine_revoked"}


def test_web_dev_machine_create_runner_offline_is_409(user_client, daemon_world, machine_flow):
    """Idle machine (no control session): fails fast, nothing enqueued."""
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/create-runner/",
        {"workspace": daemon_world["workspace"]["id"], "project": daemon_world["project"]["identifier"]},
    )
    assert response.status_code == 409
    assert response.json() == {"error": "machine_offline"}


def test_web_dev_machine_create_runner_validates_input(user_client, daemon_world, machine_flow):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    base = {"workspace": daemon_world["workspace"]["id"]}
    missing_project = web_post(
        user_client, f"{WEB}/dev-machines/{machine_id}/create-runner/", dict(base)
    )
    assert missing_project.status_code == 400
    assert missing_project.json() == {"error": "project is required"}
    bad_agent = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/create-runner/",
        dict(base, project=daemon_world["project"]["identifier"], agent="hal-9000"),
    )
    assert bad_agent.status_code == 400
    assert bad_agent.json() == {"error": "invalid_agent"}
    unknown_project = web_post(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/create-runner/",
        dict(base, project="NOPE"),
    )
    assert unknown_project.status_code == 404
    assert unknown_project.json() == {"error": "project_not_found"}


def test_web_dev_machine_create_runner_status_unknown_is_404(user_client, daemon_world, machine_flow):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    response = user_client.get(
        f"{WEB}/dev-machines/{machine_id}/create-runner/{uuid.uuid4()}/",
        params={"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 404
    assert response.json() == {"error": "unknown_request"}


def test_web_dev_machine_scope_denies_foreign_machine(user_client, seeder, daemon_world):
    """A machine id from another workspace reads as 404 (not 403)."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_machine = seeder.create_dev_machine(other_owner["id"])
    response = web_post(
        user_client,
        f"{WEB}/dev-machines/{other_machine['id']}/revoke/",
        {"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_web_dev_machine_delete_shape(user_client, daemon_world, machine_flow, seeder):
    machine_id = _machine_id(user_client, daemon_world, machine_flow)
    response = web_delete(
        user_client,
        f"{WEB}/dev-machines/{machine_id}/",
        params={"workspace": daemon_world["workspace"]["id"]},
    )
    assert response.status_code == 204, response.text
    assert (
        seeder.db.fetchval("SELECT COUNT(*) FROM dev_machine WHERE id = %s", (machine_id,))
        == 0
    )


def test_web_machine_token_ticket_shape(user_client, daemon_world):
    response = web_post(
        user_client,
        f"{WEB}/machine-tokens/{daemon_world['workspace']['id']}/tickets/",
        {"host_label": "ticket-host"},
    )
    assert response.status_code == 201, response.text
    body = response.json()
    assert set(body) == {"ticket", "expires_in_secs"}
    assert body["ticket"] and body["expires_in_secs"] == 60


def test_web_machine_token_ticket_requires_host_label(user_client, daemon_world):
    response = web_post(
        user_client, f"{WEB}/machine-tokens/{daemon_world['workspace']['id']}/tickets/", {}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "host_label is required"}


def test_web_machine_token_ticket_denies_foreign_workspace(user_client, seeder):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    response = web_post(
        user_client, f"{WEB}/machine-tokens/{other_ws['id']}/tickets/",
        {"host_label": "ticket-host"},
    )
    # Not a member: the workspace itself must not be confirmed.
    assert response.status_code == 404
    assert response.json() == {"error": "workspace not found"}
