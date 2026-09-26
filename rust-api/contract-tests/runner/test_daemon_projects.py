"""Daemon project list + desktop enroll gate.

``GET /api/v1/runner/projects/`` serves the CLI/TUI picker under three auth
modes (runner bearer, ``X-Api-Key``, session). ``POST
dev-machines/desktop-enroll/`` requires a desktop-marked session — a flag
only the cloud overlay stamps — so CE always answers 403; this file pins
that gate byte for byte instead of the unreachable 201.
"""

from __future__ import annotations

import pytest

from _harness.auth import session_post
from _harness.http import api_key_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def test_daemon_projects_shape_via_api_key(settings, daemon_world):
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.get(f"{DAEMON}/projects/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body, list) and len(body) == 1
    project = body[0]
    assert project["id"] == daemon_world["project"]["id"]
    assert project["identifier"] == daemon_world["project"]["identifier"]
    assert project["name"]
    assert project["default_pod_id"] == daemon_world["pod"]["id"]
    assert project["pod_count"] == 1
    assert project["pods"] == [
        {
            "id": daemon_world["pod"]["id"],
            "name": daemon_world["pod"]["name"],
            "is_default": True,
        }
    ]


def test_daemon_projects_shape_via_runner_bearer(machine_client, daemon_world):
    response = machine_client.get(f"{DAEMON}/projects/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert [p["id"] for p in body] == [daemon_world["project"]["id"]]


def test_daemon_projects_shape_via_machine_token(settings, machine_flow, daemon_world):
    """``mt_`` + ``X-Runner-Id`` header identities the runner (no URL id here)."""
    import httpx

    with httpx.Client(
        base_url=settings.base_url,
        timeout=30.0,
        headers={
            "Authorization": f"Bearer {machine_flow['machine_token']}",
            "X-Runner-Id": machine_flow["runner_id"],
        },
    ) as client:
        response = client.get(f"{DAEMON}/projects/")
    assert response.status_code == 200, response.text
    assert [p["id"] for p in response.json()] == [daemon_world["project"]["id"]]


def test_daemon_projects_requires_auth(anon_client):
    response = anon_client.get(f"{DAEMON}/projects/")
    assert response.status_code == 401
    assert response.json() == {"error": "authentication required"}


def test_daemon_projects_isolates_tenants(settings, seeder, daemon_world):
    """A caller sees only workspaces they belong to; чужой filter is 403."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_project = seeder.create_project(other_ws["id"])
    seeder.create_pod(other_ws["id"], other_project["id"], is_default=True)
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        body = client.get(f"{DAEMON}/projects/").json()
        assert other_project["id"] not in [p["id"] for p in body]
        forbidden = client.get(f"{DAEMON}/projects/", params={"workspace": other_ws["id"]})
        assert forbidden.status_code == 403
        assert forbidden.json() == {"error": "forbidden"}


def test_daemon_desktop_enroll_denies_anonymous(anon_client):
    # No session authenticator succeeds, so DRF reports 401 rather than the
    # permission's 403 (``permission_denied`` maps "nobody authenticated" to
    # NotAuthenticated).
    response = anon_client.post(
        f"{DAEMON}/dev-machines/desktop-enroll/",
        json={"workspace_slug": "ws", "host_label": "h"},
    )
    assert response.status_code == 401


def test_daemon_desktop_enroll_denies_web_session(user_client, daemon_world):
    """CE sessions never carry the desktop flag, so even a member gets the
    desktop gate — not the workspace lookup. This pins the gate shape."""
    response = session_post(
        user_client,
        f"{DAEMON}/dev-machines/desktop-enroll/",
        {
            "workspace_slug": daemon_world["workspace"]["slug"],
            "host_label": "my-macbook",
            "app_version": "9.9.9",
        },
    )
    assert response.status_code == 403
    assert response.json() == {
        "error": "desktop_session_required",
        "detail": "This endpoint is available to the Pi Dash desktop app.",
    }
