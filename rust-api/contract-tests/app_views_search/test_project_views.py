"""Project-level views CRUD (PIDASHCONV-87, D-29).

Routes (apps/api/pi_dash/app/urls/views.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/views/
  GET/PUT/PATCH/DELETE /api/workspaces/<slug>/projects/<project_id>/views/<pk>/
"""

import os

from conftest import session_headers

PROJECT_VIEW_KEYS = {
    "access", "archived_at", "created_at", "created_by", "deleted_at",
    "description", "display_filters", "display_properties", "filters", "id",
    "is_favorite", "is_locked", "logo_props", "name", "owned_by", "project",
    "query", "rich_filters", "sort_order", "updated_at", "updated_by",
    "workspace",
}

# POST / PATCH serialize the bare instance (no is_favorite annotation);
# list / retrieve / PUT go through the annotated queryset.
WRITE_VIEW_KEYS = PROJECT_VIEW_KEYS - {"is_favorite"}


def _views_url(tenant, project_id):
    return f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/views/"


def _view_url(tenant, project_id, pk):
    return f"{_views_url(tenant, project_id)}{pk}/"


def test_create_shape(api, seed, tenant_a, project_a, auth_a):
    r = api.post(
        _views_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "Sprint board", "filters": {}},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    try:
        assert set(body) == WRITE_VIEW_KEYS, sorted(set(body) ^ WRITE_VIEW_KEYS)
        assert body["name"] == "Sprint board"
        assert body["owned_by"] == tenant_a["user"]["id"]
        assert body["project"] == project_a
        assert body["workspace"] == tenant_a["workspace"]["id"]
        assert body["query"] == {}
        assert body["access"] == 1
        assert body["is_locked"] is False
    finally:
        api.delete(_view_url(tenant_a, project_a, body["id"]), headers=auth_a)


def test_list_shape(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Listed view",
    )
    r = api.get(_views_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list)
    entry = next(v for v in body if v["id"] == view_id)
    assert set(entry) == PROJECT_VIEW_KEYS
    assert entry["name"] == "Listed view"
    assert entry["is_favorite"] is False


def test_list_fields_param_is_ignored(api, seed, tenant_a, project_a, auth_a):
    """Port-existing-quirk: ?fields=name is accepted but the list view
    still renders full objects (DynamicBaseSerializer ignores the param
    here). Pinned so the Rust port reproduces it."""
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Field view",
    )
    r = api.get(
        _views_url(tenant_a, project_a), headers=auth_a, params={"fields": "name"}
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body, "expected at least one view"
    for entry in body:
        assert set(entry) == PROJECT_VIEW_KEYS


def test_list_marks_favorite(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Fav view",
    )
    r = api.post(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/user-favorite-views/",
        headers=auth_a,
        json={"view": str(view_id)},
    )
    assert r.status_code == 204, r.text
    try:
        r = api.get(_views_url(tenant_a, project_a), headers=auth_a)
        assert r.status_code == 200, r.text
        entry = next(v for v in r.json() if v["id"] == view_id)
        assert entry["is_favorite"] is True
    finally:
        api.delete(
            f"/api/workspaces/{tenant_a['workspace']['slug']}"
            f"/projects/{project_a}/user-favorite-views/{view_id}/",
            headers=auth_a,
        )


def test_retrieve_shape(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Detail view",
    )
    r = api.get(_view_url(tenant_a, project_a, view_id), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == PROJECT_VIEW_KEYS
    assert body["id"] == view_id
    assert body["name"] == "Detail view"


def test_retrieve_guest_sees_only_own(api, seed, tenant_a, project_a, auth_a):
    """Guest (role 5) with guest_view_all_features off cannot read another
    owner's project view: 403."""
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=20)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=5)
    other_headers = session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Owner view",
    )
    r = api.get(_view_url(tenant_a, project_a, view_id), headers=other_headers)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You are not allowed to view this issue"}


def test_partial_update(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Before",
    )
    r = api.patch(
        _view_url(tenant_a, project_a, view_id), headers=auth_a,
        json={"name": "After"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == WRITE_VIEW_KEYS, sorted(set(body) ^ WRITE_VIEW_KEYS)
    assert body["name"] == "After"


def test_full_update_shape(api, seed, tenant_a, project_a, auth_a):
    """PUT goes through the annotated queryset, so is_favorite is present
    (unlike PATCH)."""
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Put me",
    )
    r = api.put(
        _view_url(tenant_a, project_a, view_id), headers=auth_a,
        json={"name": "Put done", "filters": {}},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == PROJECT_VIEW_KEYS, sorted(set(body) ^ PROJECT_VIEW_KEYS)
    assert body["name"] == "Put done"
    assert body["is_favorite"] is False


def test_update_locked_view_rejected(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Locked", locked=True,
    )
    r = api.patch(
        _view_url(tenant_a, project_a, view_id), headers=auth_a,
        json={"name": "Unlock attempt"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "view is locked"}


def test_update_by_non_owner_rejected(api, seed, tenant_a, project_a, auth_a):
    """A public view PATCHed by a project admin who does not own it: the
    creator gate passes (admin role) but the owner check inside rejects."""
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=20)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=20)
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Mine",
    )
    other_headers = session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.patch(
        _view_url(tenant_a, project_a, view_id), headers=other_headers,
        json={"name": "Theirs"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Only the owner of the view can update the view"}


def test_destroy_owner(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Doomed",
    )
    r = api.delete(_view_url(tenant_a, project_a, view_id), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(_views_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert all(v["id"] != view_id for v in r.json())


def test_destroy_denied_for_member_non_owner(api, seed, tenant_a, project_a, auth_a):
    """Workspace member (not admin) + project member (not admin), not the
    owner: the creator gate fails (seeded row is created_by the owner),
    the admin role gate fails, and the workspace-admin bypass does not
    apply → 403."""
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=15)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=15)
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Kept",
    )
    other_headers = session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.delete(_view_url(tenant_a, project_a, view_id), headers=other_headers)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_requires_auth(api, tenant_a, project_a):
    r = api.get(_views_url(tenant_a, project_a))
    assert r.status_code == 401, r.text


def test_denied_without_project_membership(api, seed, tenant_a, project_a):
    """Workspace member with no project membership hits the PROJECT-level
    role gate → 403. This is the suite's denied-permission case."""
    outsider = seed.user()
    seed.member(tenant_a["workspace"]["id"], outsider["id"], role=20)
    headers = session_headers(
        seed, outsider, outsider["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.get(_views_url(tenant_a, project_a), headers=headers)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_tenant_isolation(api, seed, tenant_a, tenant_b, project_a, auth_a, auth_b):
    """Tenant B (no membership in A's workspace) is denied at the PROJECT
    role gate — A's views do not leak, not even as an empty list."""
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="A private view",
    )
    r = api.get(_views_url(tenant_a, project_a), headers=auth_b)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}
