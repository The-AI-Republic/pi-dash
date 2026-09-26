"""Contract tests: external integrations (Unsplash, AI assistants).

Paths (``app/urls/external.py``):
  GET  /api/unsplash/
  POST /api/workspaces/<slug>/projects/<id>/ai-assistant/
  POST /api/workspaces/<slug>/ai-assistant/

Without an ``UNSPLASH_ACCESS_KEY`` the endpoint answers ``[]`` (the keyed
path proxies api.unsplash.com and is covered by the Django test-client
suite). The GPT endpoints require an ``LLM_API_KEY`` on the server — inert
dummy in CI — so the suite pins the task-validation branch and the
provider-error envelope; the success shape needs a real provider key and is
covered client-side.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, proj_url, ws_url

pytestmark = pytest.mark.contract


def test_unsplash_without_key_returns_empty(admin):
    response = admin.get("/api/unsplash/")
    assert response.status_code == 200
    assert response.json() == []


def test_unsplash_query_params_ignored_without_key(admin):
    response = admin.get("/api/unsplash/", params={"query": "cats", "page": 2, "per_page": 5})
    assert response.status_code == 200
    assert response.json() == []


def test_workspace_assistant_requires_task(admin, world):
    response = admin.post(ws_url(world, "ai-assistant"), json={})
    assert response.status_code == 400
    assert response.json() == {"error": "Task is required"}


def test_workspace_assistant_provider_error_envelope(admin, world):
    response = admin.post(
        ws_url(world, "ai-assistant"), json={"task": "summarize", "prompt": "hello"}
    )
    assert response.status_code == 500
    assert response.json() == {"error": "An internal error has occurred."}


def test_workspace_assistant_member_denied_without_membership(outsider_client, world):
    response = outsider_client.post(ws_url(world, "ai-assistant"), json={"task": "x"})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_project_assistant_requires_task(admin, world):
    response = admin.post(proj_url(world, "ai-assistant"), json={})
    assert response.status_code == 400
    assert response.json() == {"error": "Task is required"}


def test_project_assistant_provider_error_envelope(admin, world):
    response = admin.post(
        proj_url(world, "ai-assistant"), json={"task": "summarize", "prompt": "hello"}
    )
    assert response.status_code == 500
    assert response.json() == {"error": "An internal error has occurred."}


def test_project_assistant_guest_denied(guest_client, world):
    response = guest_client.post(proj_url(world, "ai-assistant"), json={"task": "x"})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_project_assistant_cross_tenant_denied(other_admin, world):
    response = other_admin.post(proj_url(world, "ai-assistant"), json={"task": "x"})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_anonymous_rejected(anon, world):
    for response in (
        anon.get("/api/unsplash/"),
        anon.post(ws_url(world, "ai-assistant"), json={"task": "x"}),
        anon.post(proj_url(world, "ai-assistant"), json={"task": "x"}),
    ):
        assert response.status_code == 401
        assert response.json() == ANON
