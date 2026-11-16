"""View access + publish anchor write path (PIDASHCONV-1682).

Ported from apps/api/pi_dash/tests/contract/app/test_view_app.py at Django
main b4defc50f (VIEW-019, VIEW-020, NEWFRONT-972): TestProjectViewAccessWrite,
TestProjectViewAnchorWrite, TestWorkspaceViewAccessWrite. The retrieve class
of that file (VIEW-022) belongs to PIDASHCONV-1683 and is not ported here.

The owner may store `access` on either scope and the publish `anchor` (both
go through the shared IssueViewSerializer, so the workspace scope accepts
`anchor` too); the PATCH echo carries the stored value and a follow-up GET
reports it persisted. Non-owners cannot write: project scope refuses with
the owner error, workspace scope with the creator-gated 403.

Routes (apps/api/pi_dash/app/urls/views.py):
  POST /api/workspaces/<slug>/projects/<project_id>/views/
  GET/PATCH /api/workspaces/<slug>/projects/<project_id>/views/<pk>/
  GET/PATCH /api/workspaces/<slug>/views/<pk>/
"""

import os
from uuid import uuid4

from .conftest import session_headers

OWNER_ERROR = {"error": "Only the owner of the view can update the view"}
PERMISSION_ERROR = {"error": "You don't have the required permissions."}
DUPLICATE_ANCHOR = {"anchor": ["Issue View with this anchor already exists."]}
INVALID_ACCESS = {"access": ['"7" is not a valid choice.']}


def _project_view_url(tenant, project_id, view_id):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{project_id}/views/{view_id}/"
    )


def _project_views_url(tenant, project_id):
    return f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/views/"


def _workspace_view_url(tenant, view_id):
    return f"/api/workspaces/{tenant['workspace']['slug']}/views/{view_id}/"


def _member_headers(seed, tenant, project_id):
    """A workspace admin (role 20) + project member (role 15) who owns no
    views: on the project scope the decorator lets members through and the
    owner check refuses them; on the workspace scope the creator gate
    refuses them first (mirrors the Django `member_user` fixture)."""
    other = seed.user()
    seed.member(tenant["workspace"]["id"], other["id"], role=20)
    if project_id is not None:
        seed.project_member(
            project_id, tenant["workspace"]["id"], other["id"], role=15
        )
    return session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )


def _anchor_follows_access(body):
    keys = list(body.keys())
    assert keys.index("anchor") == keys.index("access") + 1, keys


def test_owner_patch_access_echoes_and_persists(
    api, seed, tenant_a, project_a, auth_a
):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    url = _project_view_url(tenant_a, project_a, view_id)
    assert api.get(url, headers=auth_a).json()["access"] == 1

    r = api.patch(url, headers=auth_a, json={"access": 0})
    assert r.status_code == 200, r.text
    assert r.json()["access"] == 0
    assert api.get(url, headers=auth_a).json()["access"] == 0


def test_create_without_access_keeps_default(api, seed, tenant_a, project_a, auth_a):
    r = api.post(
        _project_views_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "Fresh view"},
    )
    assert r.status_code == 201, r.text
    try:
        assert r.json()["access"] == 1
        assert r.json()["anchor"] is None
    finally:
        api.delete(
            _project_view_url(tenant_a, project_a, r.json()["id"]),
            headers=auth_a,
        )


def test_create_with_access_and_anchor_stores(api, seed, tenant_a, project_a, auth_a):
    """Extension beyond the Django file: create accepts both new
    writables (the shared serializer exposes them on every write path)."""
    r = api.post(
        _project_views_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "Born private", "access": 0, "anchor": "born-anchor-1"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    try:
        assert body["access"] == 0
        assert body["anchor"] == "born-anchor-1"
        _anchor_follows_access(body)
        got = api.get(
            _project_view_url(tenant_a, project_a, body["id"]), headers=auth_a
        ).json()
        assert got["access"] == 0
        assert got["anchor"] == "born-anchor-1"
    finally:
        api.delete(
            _project_view_url(tenant_a, project_a, body["id"]), headers=auth_a
        )


def test_non_owner_patch_access_is_refused(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    headers = _member_headers(seed, tenant_a, project_a)
    url = _project_view_url(tenant_a, project_a, view_id)

    r = api.patch(url, headers=headers, json={"access": 0})
    assert r.status_code == 400, r.text
    assert r.json() == OWNER_ERROR
    assert api.get(url, headers=auth_a).json()["access"] == 1


def test_invalid_access_is_rejected(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    url = _project_view_url(tenant_a, project_a, view_id)

    r = api.patch(url, headers=auth_a, json={"access": 7})
    assert r.status_code == 400, r.text
    assert "access" in r.json()
    assert r.json() == INVALID_ACCESS
    assert api.get(url, headers=auth_a).json()["access"] == 1


def test_owner_publish_echoes_and_persists(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    url = _project_view_url(tenant_a, project_a, view_id)
    assert api.get(url, headers=auth_a).json()["anchor"] is None

    r = api.patch(url, headers=auth_a, json={"anchor": "view-anchor-1"})
    assert r.status_code == 200, r.text
    assert r.json()["anchor"] == "view-anchor-1"
    _anchor_follows_access(r.json())
    body = api.get(url, headers=auth_a).json()
    assert body["anchor"] == "view-anchor-1"
    _anchor_follows_access(body)


def test_owner_unpublish_clears(api, seed, tenant_a, project_a, auth_a):
    # A second unpublished view shares anchor=None, proving the clear is
    # not rejected as a uniqueness conflict.
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Other view",
    )
    url = _project_view_url(tenant_a, project_a, view_id)
    r = api.patch(url, headers=auth_a, json={"anchor": "view-anchor-1"})
    assert r.status_code == 200, r.text

    r = api.patch(url, headers=auth_a, json={"anchor": None})
    assert r.status_code == 200, r.text
    assert r.json()["anchor"] is None
    assert api.get(url, headers=auth_a).json()["anchor"] is None


def test_duplicate_anchor_is_rejected(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    url = _project_view_url(tenant_a, project_a, view_id)
    r = api.patch(url, headers=auth_a, json={"anchor": "view-anchor-1"})
    assert r.status_code == 200, r.text
    other_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Other view",
    )
    other_url = _project_view_url(tenant_a, project_a, other_id)

    r = api.patch(other_url, headers=auth_a, json={"anchor": "view-anchor-1"})
    assert r.status_code == 400, r.text
    assert "anchor" in r.json()
    assert r.json() == DUPLICATE_ANCHOR
    assert api.get(other_url, headers=auth_a).json()["anchor"] is None


def test_republish_same_anchor_is_idempotent(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    url = _project_view_url(tenant_a, project_a, view_id)

    first = api.patch(url, headers=auth_a, json={"anchor": "view-anchor-1"})
    second = api.patch(url, headers=auth_a, json={"anchor": "view-anchor-1"})
    assert first.status_code == 200, first.text
    assert second.status_code == 200, second.text
    assert second.json()["anchor"] == "view-anchor-1"


def test_non_owner_publish_is_refused(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Active issues",
    )
    headers = _member_headers(seed, tenant_a, project_a)
    url = _project_view_url(tenant_a, project_a, view_id)

    r = api.patch(url, headers=headers, json={"anchor": "view-anchor-1"})
    assert r.status_code == 400, r.text
    assert r.json() == OWNER_ERROR
    assert api.get(url, headers=auth_a).json()["anchor"] is None


def test_workspace_owner_patch_access_echoes_and_persists(
    api, seed, tenant_a, auth_a
):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        name="Workspace triage",
    )
    url = _workspace_view_url(tenant_a, view_id)
    assert api.get(url, headers=auth_a).json()["access"] == 1

    r = api.patch(url, headers=auth_a, json={"access": 0})
    assert r.status_code == 200, r.text
    assert r.json()["access"] == 0
    assert api.get(url, headers=auth_a).json()["access"] == 0


def test_workspace_owner_publish_echoes_and_persists(api, seed, tenant_a, auth_a):
    """Extension beyond the Django file: the workspace scope shares the
    same serializer, so `anchor` writes there too (the issue requires
    both scopes)."""
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        name="Workspace triage",
    )
    url = _workspace_view_url(tenant_a, view_id)

    r = api.patch(url, headers=auth_a, json={"anchor": "ws-anchor-1"})
    assert r.status_code == 200, r.text
    assert r.json()["anchor"] == "ws-anchor-1"
    body = api.get(url, headers=auth_a).json()
    assert body["anchor"] == "ws-anchor-1"
    _anchor_follows_access(body)


def test_workspace_non_owner_patch_access_is_refused(
    api, seed, tenant_a, project_a, auth_a
):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        name="Workspace triage",
    )
    headers = _member_headers(seed, tenant_a, project_a)
    url = _workspace_view_url(tenant_a, view_id)

    r = api.patch(url, headers=headers, json={"access": 0})
    assert r.status_code == 403, r.text
    assert r.json() == PERMISSION_ERROR
    assert api.get(url, headers=auth_a).json()["access"] == 1


def test_workspace_missing_view_initial_body(api, seed, tenant_a, auth_a):
    """Extension beyond the Django file: the B1 missing-row body is the
    serializer's `get_initial()`, which gained the newly writable
    `access` (null) and `anchor` ("") under NEWFRONT-972 (probed)."""
    url = _workspace_view_url(tenant_a, uuid4())
    r = api.get(url, headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json() == {
        "deleted_at": None, "name": "", "description": "", "filters": None,
        "display_filters": None, "display_properties": None,
        "rich_filters": None, "access": None, "anchor": "",
        "sort_order": None, "logo_props": None, "archived_at": None,
        "created_by": None, "updated_by": None,
    }
