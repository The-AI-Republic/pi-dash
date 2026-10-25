"""Contract tests: external integrations (AI assistants).

Paths (``app/urls/external.py``):
  POST /api/workspaces/<slug>/projects/<id>/ai-assistant/
  POST /api/workspaces/<slug>/ai-assistant/

``GET /api/unsplash/`` was removed with the cover-image feature
(PDASHOSS01-237): it matches no URL pattern, so Django answers its resolver
404 before authentication, and Rust registers no route for it (the edge
proxies the miss to Django). The GPT endpoints require an ``LLM_API_KEY`` on the server — inert
dummy in CI — so the suite pins the task-validation branch and the
provider-error envelope; the success shape needs a real provider key and is
covered client-side.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, proj_url, ws_url

pytestmark = pytest.mark.contract


def _assert_resolver_404(response):
    # The harness serves Django with ``DEBUG = True`` (technical HTML 404);
    # production ``handler404`` answers ``{"error": "Page not found."}``.
    assert response.status_code == 404, response.text[:200]
    content_type = response.headers.get("content-type", "")
    if content_type.startswith("application/json"):
        assert response.json() == {"error": "Page not found."}, response.text
    else:
        assert content_type.startswith("text/html"), content_type


@pytest.mark.parametrize("params", [None, {"query": "cats", "page": 2, "per_page": 5}])
def test_unsplash_route_removed(admin, params):
    _assert_resolver_404(admin.get("/api/unsplash/", params=params))


def test_unsplash_route_removed_anonymous(anon):
    # The resolver 404 precedes authentication.
    _assert_resolver_404(anon.get("/api/unsplash/"))


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
        anon.post(ws_url(world, "ai-assistant"), json={"task": "x"}),
        anon.post(proj_url(world, "ai-assistant"), json={"task": "x"}),
    ):
        assert response.status_code == 401
        assert response.json() == ANON


def test_unknown_project_identifier_404(admin, world):
    # A non-UUID project identifier that resolves to nothing answers 404
    # {"detail": "Project not found"}: Project.resolve raises
    # Http404("Project not found") and DRF's exception_handler preserves
    # the message. (PIDASHCONV-465.)
    bad = (
        f"/api/workspaces/{world['workspace']['slug']}"
        f"/projects/NO-SUCH-PROJECT/ai-assistant/"
    )
    response = admin.post(bad, json={"task": "x"})
    assert response.status_code == 404, response.status_code
    assert response.json() == {"detail": "Project not found"}, (
        response.text[:300])
