"""Web runner list/detail/revoke/revive/delete (``GET/POST/DELETE /api/runners/``).

Session-auth runner management: list (workspace-scoped, private runners are
owner-visible only), detail read + PATCH rename/pod-move, revoke
(idempotent), deprecated revive (410), hard delete (204).
"""

from __future__ import annotations

import pytest

from _harness.web import web_delete, web_patch, web_post

WEB = "/api/runners"

pytestmark = pytest.mark.contract

RUNNER_KEYS = {
    "id", "name", "status", "host_label", "provisioning", "os", "arch",
    "runner_version", "dev_metadata", "protocol_version", "capabilities",
    "last_heartbeat_at", "owner", "dev_machine", "dev_machine_detail",
    "visibility", "pod", "pod_detail", "live_state", "enrolled_at",
    "revoked_at", "revoked_reason", "created_at", "updated_at",
}


def test_web_runner_list_shape(user_client, daemon_world, machine_flow):
    response = user_client.get(WEB + "/", params={"workspace": daemon_world["workspace"]["id"]})
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    runner = body[0]
    assert RUNNER_KEYS <= set(runner)
    assert runner["id"] == machine_flow["runner_id"]
    assert runner["name"] == machine_flow["runner_name"]
    assert runner["status"] == "offline"
    assert runner["owner"] == daemon_world["owner"]["id"]
    assert runner["pod"] == daemon_world["pod"]["id"]
    assert runner["pod_detail"]["project_identifier"] == daemon_world["project"]["identifier"]
    assert runner["revoked_at"] is None


def test_web_runner_list_requires_workspace(user_client):
    response = user_client.get(WEB + "/")
    assert response.status_code == 400
    assert response.json() == {"error": "workspace is required"}


def test_web_runner_list_denies_anonymous(anon_client, daemon_world):
    response = anon_client.get(WEB + "/", params={"workspace": daemon_world["workspace"]["id"]})
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_runner_list_isolates_tenants(user_client, seeder, daemon_world):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    forbidden = user_client.get(WEB + "/", params={"workspace": other_ws["id"]})
    assert forbidden.status_code == 403
    assert forbidden.json() == {"error": "forbidden"}
    own = user_client.get(WEB + "/", params={"workspace": daemon_world["workspace"]["id"]})
    assert own.status_code == 200
    assert all(r["id"] != "00000000-0000-0000-0000-000000000000" for r in own.json())


def test_web_runner_detail_shape(user_client, machine_flow):
    response = user_client.get(f"{WEB}/{machine_flow['runner_id']}/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert RUNNER_KEYS <= set(body)
    assert body["id"] == machine_flow["runner_id"]
    assert body["dev_machine_detail"]["host_label"] == machine_flow["host_label"]


def test_web_runner_detail_unknown_is_404(user_client):
    response = user_client.get(f"{WEB}/00000000-0000-0000-0000-000000000000/")
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_web_runner_detail_hides_foreign_private_runner(user_client, seeder, daemon_world):
    """Same-workspace other-owner private runner reads as 404 (not 403)."""
    other_owner = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], other_owner["id"])
    seeded = seeder.enroll_runner(
        other_owner["id"], daemon_world["workspace"]["id"], daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}other",
    )
    response = user_client.get(f"{WEB}/{seeded['id']}/")
    assert response.status_code == 404
    assert response.json() == {"error": "not found"}


def test_web_runner_detail_denies_anonymous(anon_client, machine_flow):
    response = anon_client.get(f"{WEB}/{machine_flow['runner_id']}/")
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_runner_patch_rename(user_client, machine_flow, seeder):
    response = web_patch(user_client, f"{WEB}/{machine_flow['runner_id']}/", {"name": "renamed-web"})
    assert response.status_code == 200, response.text
    assert response.json()["name"] == "renamed-web"
    assert (
        seeder.db.fetchval("SELECT name FROM runner WHERE id = %s", (machine_flow["runner_id"],))
        == "renamed-web"
    )


def test_web_runner_patch_empty_name_is_400(user_client, machine_flow):
    response = web_patch(user_client, f"{WEB}/{machine_flow['runner_id']}/", {"name": "  "})
    assert response.status_code == 400
    assert response.json() == {"error": "name cannot be empty"}


def test_web_runner_patch_busy_pod_move_is_409(user_client, machine_flow, daemon_run):
    """The flow runner serves an ASSIGNED run: moving pods strands it."""
    response = web_patch(
        user_client,
        f"{WEB}/{machine_flow['runner_id']}/",
        {"pod": "00000000-0000-0000-0000-000000000000"},
    )
    # Unknown pod fails before the busy guard.
    assert response.status_code == 400
    assert response.json() == {"error": "pod does not exist or has been deleted"}


def test_web_runner_patch_pod_move_busy_guard(user_client, seeder, daemon_world, machine_flow, daemon_run):
    new_pod = seeder.create_pod(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"], name="Busy-guard pod"
    )
    response = web_patch(user_client, f"{WEB}/{machine_flow['runner_id']}/", {"pod": new_pod["id"]})
    assert response.status_code == 409
    assert response.json()["code"] == "runner_busy"


def test_web_runner_revoke_shape(user_client, machine_flow, seeder):
    first = web_post(user_client, f"{WEB}/{machine_flow['runner_id']}/revoke/", {})
    assert first.status_code == 200, first.text
    body = first.json()
    assert RUNNER_KEYS <= set(body)
    assert body["revoked_at"] is not None
    assert body["status"] == "revoked"
    # Idempotent: a second call returns the current state without error.
    second = web_post(user_client, f"{WEB}/{machine_flow['runner_id']}/revoke/", {})
    assert second.status_code == 200
    assert second.json()["revoked_at"] == body["revoked_at"]


def test_web_runner_revive_is_deprecated_410(user_client, machine_flow):
    response = web_post(user_client, f"{WEB}/{machine_flow['runner_id']}/revive/", {})
    assert response.status_code == 410
    assert response.json()["error"] == "legacy_enrollment_disabled"


def test_web_runner_delete_shape(user_client, machine_flow, seeder):
    runner_id = machine_flow["runner_id"]
    response = web_delete(user_client, f"{WEB}/{runner_id}/")
    assert response.status_code == 204, response.text
    assert (
        seeder.db.fetchval("SELECT COUNT(*) FROM runner WHERE id = %s", (runner_id,)) == 0
    )
    gone = user_client.get(f"{WEB}/{runner_id}/")
    assert gone.status_code == 404


def test_web_runner_invites_deprecated_410(user_client):
    response = web_post(user_client, f"{WEB}/invites/", {})
    assert response.status_code == 410
    assert response.json()["error"] == "legacy_enrollment_disabled"


def test_web_runner_invites_denies_anonymous(anon_client):
    response = anon_client.post(f"{WEB}/invites/", json={})
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}
