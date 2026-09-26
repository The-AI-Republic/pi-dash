"""Web pods + projects catalog (``/api/runners/pods/``, ``/api/runners/projects/``).

Pods: workspace/project-scoped list, admin-only create, rename/toggle,
guarded soft-delete (runners / active runs / default-pod guards). Projects:
read-only picker served under three auth modes (runner bearer, ``X-Api-Key``,
session) — the one endpoint on this surface with ``permission_classes = []``.
"""

from __future__ import annotations

import pytest

from contextlib import contextmanager

from _harness.auth import login_session
from _harness.http import api_client, api_key_client
from _harness.web import web_delete, web_patch, web_post

WEB = "/api/runners"

pytestmark = pytest.mark.contract

POD_KEYS = {
    "id", "name", "description", "is_default", "workspace", "project",
    "project_identifier", "created_by", "runner_count", "created_at", "updated_at",
}

PROJECT_KEYS = {
    "id", "identifier", "name", "description", "is_default",
    "default_pod_id", "pod_count", "pods",
}


@contextmanager
def _admin_client(settings, seeder, daemon_world):
    """Logged-in session client for a workspace admin (pod manage needs admin)."""
    admin = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], admin["id"], role=20)
    with api_client(settings.base_url) as client:
        login_session(client, email=admin["email"], password=admin["password"])
        yield client


def test_web_pod_list_shape(user_client, daemon_world, machine_flow):
    response = user_client.get(
        f"{WEB}/pods/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    pod = body[0]
    assert POD_KEYS <= set(pod)
    assert pod["id"] == daemon_world["pod"]["id"]
    assert pod["is_default"] is True
    assert pod["project"] == daemon_world["project"]["id"]
    assert pod["project_identifier"] == daemon_world["project"]["identifier"]
    assert pod["runner_count"] == 1


def test_web_pod_list_by_project(user_client, daemon_world):
    response = user_client.get(
        f"{WEB}/pods/", params={"project": daemon_world["project"]["id"]}
    )
    assert response.status_code == 200, response.text
    assert [p["id"] for p in response.json()] == [daemon_world["pod"]["id"]]


def test_web_pod_list_requires_scope(user_client):
    response = user_client.get(f"{WEB}/pods/")
    assert response.status_code == 400
    assert response.json() == {"error": "project or workspace is required"}


def test_web_pod_list_denies_anonymous(anon_client, daemon_world):
    response = anon_client.get(
        f"{WEB}/pods/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 401
    assert response.json() == {"detail": "Authentication credentials were not provided."}


def test_web_pod_list_isolates_tenants(user_client, seeder, daemon_world):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    forbidden = user_client.get(f"{WEB}/pods/", params={"workspace": other_ws["id"]})
    assert forbidden.status_code == 403
    assert forbidden.json() == {"error": "forbidden"}


def test_web_pod_detail_shape(user_client, daemon_world):
    response = user_client.get(f"{WEB}/pods/{daemon_world['pod']['id']}/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert POD_KEYS <= set(body)
    assert body["name"] == daemon_world["pod"]["name"]


def test_web_pod_create_shape(settings, seeder, daemon_world):
    with _admin_client(settings, seeder, daemon_world) as client:
        response = web_post(
            client,
            f"{WEB}/pods/",
            {"project": daemon_world["project"]["id"], "name": "web-contract"},
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert POD_KEYS <= set(body)
    # Bare suffix is re-prefixed with the project identifier.
    assert body["name"] == f"{daemon_world['project']['identifier']}_web-contract"
    assert body["is_default"] is False
    assert body["project"] == daemon_world["project"]["id"]


def test_web_pod_create_denies_member(user_client, daemon_world):
    response = web_post(
        user_client, f"{WEB}/pods/",
        {"project": daemon_world["project"]["id"], "name": "member-pod"},
    )
    assert response.status_code == 403
    assert response.json() == {"error": "workspace admin required"}


def test_web_pod_rename_shape(settings, seeder, daemon_world):
    with _admin_client(settings, seeder, daemon_world) as client:
        response = web_patch(
            client,
            f"{WEB}/pods/{daemon_world['pod']['id']}/",
            {"description": "renamed by web"},
        )
    assert response.status_code == 200, response.text
    assert response.json()["description"] == "renamed by web"


def test_web_pod_rename_denies_member(user_client, daemon_world):
    response = web_patch(
        user_client, f"{WEB}/pods/{daemon_world['pod']['id']}/",
        {"description": "member rename"},
    )
    assert response.status_code == 403
    assert response.json() == {"error": "forbidden"}


def test_web_pod_delete_denies_member(user_client, daemon_world, machine_flow):
    """Pod manage needs admin-or-creator: a plain member gets 403, not 409."""
    response = web_delete(user_client, f"{WEB}/pods/{daemon_world['pod']['id']}/")
    assert response.status_code == 403
    assert response.json() == {"error": "forbidden"}


def test_web_pod_delete_guards_default_with_runners(settings, seeder, daemon_world, machine_flow):
    """Default pod holding runners: the runner guard fires first."""
    with _admin_client(settings, seeder, daemon_world) as client:
        response = web_delete(client, f"{WEB}/pods/{daemon_world['pod']['id']}/")
    assert response.status_code == 409
    assert response.json()["code"] == "pod_has_runners"


def test_web_pod_delete_shape(settings, seeder, daemon_world):
    with _admin_client(settings, seeder, daemon_world) as client:
        created = web_post(
            client, f"{WEB}/pods/",
            {"project": daemon_world["project"]["id"], "name": "doomed"},
        )
        assert created.status_code == 201, created.text
        pod_id = created.json()["id"]
        deleted = web_delete(client, f"{WEB}/pods/{pod_id}/")
    assert deleted.status_code == 204, deleted.text
    gone = seeder.db.fetchval("SELECT COUNT(*) FROM pod WHERE id = %s AND deleted_at IS NULL", (pod_id,))
    assert gone == 0


def test_web_projects_shape_via_session(user_client, daemon_world):
    response = user_client.get(
        f"{WEB}/projects/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    project = body[0]
    assert PROJECT_KEYS <= set(project)
    assert project["id"] == daemon_world["project"]["id"]
    assert project["identifier"] == daemon_world["project"]["identifier"]
    assert project["default_pod_id"] == daemon_world["pod"]["id"]
    assert project["pod_count"] == 1
    assert project["pods"] == [
        {"id": daemon_world["pod"]["id"], "name": daemon_world["pod"]["name"], "is_default": True}
    ]


def test_web_projects_shape_via_api_key(settings, daemon_world):
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.get(
            f"{WEB}/projects/", params={"workspace": daemon_world["workspace"]["id"]}
        )
    assert response.status_code == 200, response.text
    assert [p["id"] for p in response.json()] == [daemon_world["project"]["id"]]


def test_web_projects_requires_auth(anon_client):
    # Explicit 401 (not DRF's default 403): permission_classes is empty and
    # the view answers "authentication required" itself.
    response = anon_client.get(f"{WEB}/projects/")
    assert response.status_code == 401
    assert response.json() == {"error": "authentication required"}


def test_web_projects_isolates_tenants(user_client, seeder, daemon_world):
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    forbidden = user_client.get(f"{WEB}/projects/", params={"workspace": other_ws["id"]})
    assert forbidden.status_code == 403
    assert forbidden.json() == {"error": "forbidden"}
    own = user_client.get(
        f"{WEB}/projects/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert [p["id"] for p in own.json()] == [daemon_world["project"]["id"]]
