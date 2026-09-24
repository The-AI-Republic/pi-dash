"""Contract tests: workspace GitHub PAT integration (connect/disconnect/status/repos).

Paths (``app/urls/integration.py``):
  GET  /api/workspaces/<slug>/integrations/github/
  POST /api/workspaces/<slug>/integrations/github/connect/
  POST /api/workspaces/<slug>/integrations/github/disconnect/
  GET  /api/workspaces/<slug>/integrations/github/repos/

The live-GitHub paths are pinned through their deterministic error shapes
(``connect`` with a bogus token is rejected upstream with 401); the
connected-state shapes come from SQL-seeded ``workspace_integrations`` rows so
no test needs a real credential.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, ws_url

pytestmark = pytest.mark.contract


def test_status_not_connected_shape(admin, world):
    response = admin.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_status_connected_shape(admin, world, seeder):
    seeder.create_workspace_integration(
        world["workspace"]["id"], world["owner"]["id"], connected=True
    )
    response = admin.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {
        "connected": True,
        "github_user_login": "contract-octocat",
        "verified_at": "2026-01-01T00:00:00+00:00",
    }


def test_status_member_reads(member_client, world):
    response = member_client.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_status_guest_denied(guest_client, world):
    response = guest_client.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_connect_missing_token(admin, world):
    response = admin.post(ws_url(world, "integrations", "github", "connect"), json={})
    assert response.status_code == 400
    assert response.json() == {"error": "GitHub PAT required"}


def test_connect_bogus_token_rejected_upstream(admin, world):
    response = admin.post(
        ws_url(world, "integrations", "github", "connect"),
        json={"token": "ghp_boguscontracttesttoken0000000000"},
    )
    assert response.status_code == 401
    assert response.json() == {"error": "GitHub rejected this token"}


def test_connect_member_denied(member_client, world):
    response = member_client.post(
        ws_url(world, "integrations", "github", "connect"), json={"token": "x"}
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_disconnect_never_connected(admin, world):
    response = admin.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_disconnect_connected_clears_credential(admin, world, seeder):
    seeder.create_workspace_integration(
        world["workspace"]["id"], world["owner"]["id"], connected=True
    )
    response = admin.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}

    status = admin.get(ws_url(world, "integrations", "github"))
    assert status.json() == {"connected": False}

    repos = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert repos.status_code == 409
    assert repos.json() == {"error": "GitHub credential is missing"}


def test_disconnect_member_denied(member_client, world):
    response = member_client.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_repos_not_connected(admin, world):
    response = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 404
    assert response.json() == {"error": "GitHub not connected"}


def test_repos_credential_missing_after_row_without_token(admin, world, seeder):
    seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    response = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 409
    assert response.json() == {"error": "GitHub credential is missing"}


def test_repos_member_denied_without_membership(outsider_client, world):
    response = outsider_client.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_anonymous_rejected(anon, world):
    for response in (
        anon.get(ws_url(world, "integrations", "github")),
        anon.post(ws_url(world, "integrations", "github", "connect"), json={}),
        anon.post(ws_url(world, "integrations", "github", "disconnect")),
        anon.get(ws_url(world, "integrations", "github", "repos")),
    ):
        assert response.status_code == 401
        assert response.json() == ANON
