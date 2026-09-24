"""Contract tests: project-level GitHub binding (bind/status/toggle/unbind).

Paths (``app/urls/integration.py``):
  POST   /api/workspaces/<slug>/projects/<id>/github/bind/
  GET    /api/workspaces/<slug>/projects/<id>/github/
  PATCH  /api/workspaces/<slug>/projects/<id>/github/
  DELETE /api/workspaces/<slug>/projects/<id>/github/

Bind verification calls api.github.com, so the suite pins the offline
branches (validation 400s, not-connected 409, permission 403s) plus the full
status/toggle/unbind lifecycle on SQL-seeded ``github_repository_syncs``
rows. The bind-success shape is covered by the Django test-client suite with
a mocked client; the success-path column contract is the seeded row itself.
"""

from __future__ import annotations

import pytest

from .conftest import DENIED, proj_url

pytestmark = pytest.mark.contract


def test_bind_missing_url(admin, world):
    response = admin.post(proj_url(world, "github", "bind"), json={})
    assert response.status_code == 400
    assert response.json() == {"error": "repo_url is required"}


def test_bind_non_github_url(admin, world):
    response = admin.post(
        proj_url(world, "github", "bind"), json={"repo_url": "https://gitlab.com/o/r"}
    )
    assert response.status_code == 400
    assert response.json() == {
        "error": "Only github.com URLs are supported (e.g. https://github.com/owner/repo)"
    }


def test_bind_not_connected(admin, world):
    response = admin.post(
        proj_url(world, "github", "bind"), json={"repo_url": "https://github.com/o/r"}
    )
    assert response.status_code == 409
    assert response.json() == {"error": "Workspace GitHub integration is not connected"}


def test_bind_member_denied(member_client, world):
    response = member_client.post(
        proj_url(world, "github", "bind"), json={"repo_url": "https://github.com/o/r"}
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_bind_cross_tenant_project_denied(other_admin, world, other_world):
    response = other_admin.post(
        proj_url(world, "github", "bind"), json={"repo_url": "https://github.com/o/r"}
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_status_unbound(admin, world):
    response = admin.get(proj_url(world, "github"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_status_cross_tenant_project_does_not_leak(admin, world, other_world):
    # PROJECT-level permission runs before the view: a project from another
    # tenant is denied, never leaked.
    response = admin.get(
        f"/api/workspaces/{world['workspace']['slug']}"
        f"/projects/{other_world['project']['id']}/github/"
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_status_bound_shape(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    sync = seeder.create_github_repo_sync(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], wi["id"]
    )
    response = admin.get(proj_url(world, "github"))
    assert response.status_code == 200
    assert response.json() == {
        "bound": True,
        "id": sync["id"],
        "repository": {
            "id": sync["repository_id"],
            "owner": sync["owner"],
            "name": sync["name"],
            "url": sync["url"],
        },
        "is_sync_enabled": False,
        "last_synced_at": None,
        "last_sync_error": "",
    }


def test_status_guest_reads(guest_client, world):
    response = guest_client.get(proj_url(world, "github"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_patch_requires_bool(admin, world):
    response = admin.patch(proj_url(world, "github"), json={"enabled": "yes"})
    assert response.status_code == 400
    assert response.json() == {"error": "enabled (bool) is required"}


def test_patch_without_binding(admin, world):
    response = admin.patch(proj_url(world, "github"), json={"enabled": True})
    assert response.status_code == 404
    assert response.json() == {"error": "No GitHub binding for this project"}


def test_patch_toggle_roundtrip(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    seeder.create_github_repo_sync(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], wi["id"]
    )
    enabled = admin.patch(proj_url(world, "github"), json={"enabled": True})
    assert enabled.status_code == 200
    assert enabled.json() == {"is_sync_enabled": True}
    assert admin.get(proj_url(world, "github")).json()["is_sync_enabled"] is True

    disabled = admin.patch(proj_url(world, "github"), json={"enabled": False})
    assert disabled.json() == {"is_sync_enabled": False}


def test_patch_member_denied(member_client, world):
    response = member_client.patch(proj_url(world, "github"), json={"enabled": True})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_delete_unbound(admin, world):
    response = admin.delete(proj_url(world, "github"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_delete_bound_unbinds(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    seeder.create_github_repo_sync(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], wi["id"]
    )
    response = admin.delete(proj_url(world, "github"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}
    assert admin.get(proj_url(world, "github")).json() == {"bound": False}


def test_delete_member_denied(member_client, world):
    response = member_client.delete(proj_url(world, "github"))
    assert response.status_code == 403
    assert response.json() == DENIED
