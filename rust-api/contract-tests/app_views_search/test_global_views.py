"""Global views + view issues + favorites (PIDASHCONV-87, D-29).

Routes (apps/api/pi_dash/app/urls/views.py):
  GET/POST /api/workspaces/<slug>/views/
  GET/PUT/PATCH/DELETE /api/workspaces/<slug>/views/<pk>/
  GET /api/workspaces/<slug>/issues/
  GET/POST /api/workspaces/<slug>/projects/<project_id>/user-favorite-views/
  DELETE /api/workspaces/<slug>/projects/<project_id>/user-favorite-views/<view_id>/
"""

import json
import os
from datetime import datetime, timedelta, timezone

from .conftest import session_headers

GLOBAL_VIEW_KEYS = {
    "access", "archived_at", "created_at", "created_by", "deleted_at",
    "description", "display_filters", "display_properties", "filters", "id",
    "is_locked", "logo_props", "name", "owned_by", "project", "query",
    "rich_filters", "sort_order", "updated_at", "updated_by", "workspace",
}

VIEW_ISSUE_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at", "estimate_point",
    "priority", "start_date", "target_date", "sequence_id", "project_id",
    "parent_id", "cycle_id", "sub_issues_count", "created_at", "updated_at",
    "created_by", "updated_by", "attachment_count", "link_count", "is_draft",
    "archived_at", "state__group", "assignee_ids", "label_ids", "module_ids",
}

PAGINATED_KEYS = {
    "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
    "prev_cursor", "next_page_results", "prev_page_results", "count",
    "total_pages", "total_results", "extra_stats", "results",
}


def _global_url(tenant):
    return f"/api/workspaces/{tenant['workspace']['slug']}/views/"


def test_global_create_shape(api, seed, tenant_a, auth_a):
    r = api.post(
        _global_url(tenant_a), headers=auth_a,
        json={"name": "Global board", "filters": {}},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    try:
        assert set(body) == GLOBAL_VIEW_KEYS, sorted(set(body) ^ GLOBAL_VIEW_KEYS)
        assert body["name"] == "Global board"
        assert body["project"] is None
        assert body["owned_by"] == tenant_a["user"]["id"]
    finally:
        api.delete(f"{_global_url(tenant_a)}{body['id']}/", headers=auth_a)


def test_global_list_shape(api, seed, tenant_a, auth_a):
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="G-listed"
    )
    r = api.get(_global_url(tenant_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list) and body
    for entry in body:
        assert set(entry) == GLOBAL_VIEW_KEYS
        assert entry["project"] is None


def test_global_retrieve_shape(api, seed, tenant_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="G-detail"
    )
    r = api.get(f"{_global_url(tenant_a)}{view_id}/", headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == GLOBAL_VIEW_KEYS
    assert body["id"] == view_id


def test_global_guest_sees_only_own(api, seed, tenant_a, auth_a):
    """Workspace guest (role 5) lists only their own global views."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    guest_headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Admin global"
    )
    own_id = seed.issue_view(
        tenant_a["workspace"]["id"], guest["id"], name="Guest global"
    )
    r = api.get(_global_url(tenant_a), headers=guest_headers)
    assert r.status_code == 200, r.text
    ids = [v["id"] for v in r.json()]
    assert ids == [own_id], ids


def test_global_partial_update(api, seed, tenant_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="G-before"
    )
    r = api.patch(
        f"{_global_url(tenant_a)}{view_id}/", headers=auth_a,
        json={"name": "G-after"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["name"] == "G-after"


def test_global_destroy_by_owner_member(api, seed, db, tenant_a, auth_a):
    """A workspace member (not admin) deletes their own global view: the
    creator gate passes and owned_by matches → 204."""
    member = seed.user()
    seed.member(tenant_a["workspace"]["id"], member["id"], role=15)
    member_headers = session_headers(
        seed, member, member["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], member["id"], name="Member global"
    )
    r = api.delete(f"{_global_url(tenant_a)}{view_id}/", headers=member_headers)
    assert r.status_code == 204, r.text
    # PIDASHCONV-925: destroy goes through the model soft-delete, which
    # runs a full save() stamping updated_by/updated_at (db/mixins.py:72-78,
    # db/models/base.py:24-44) — not a bare SET deleted_at. The seeded row
    # was never updated, so the stamp must come from the destroy itself.
    with db.cursor() as cur:
        cur.execute(
            "SELECT updated_by_id, deleted_at FROM issue_views WHERE id = %s",
            (view_id,),
        )
        row = cur.fetchone()
    assert row[1] is not None
    assert str(row[0]) == member["id"]


def test_global_destroy_denied_for_member_non_owner(api, seed, tenant_a, auth_a):
    """Workspace member (not admin), not the owner: the creator gate fails
    and the WORKSPACE role gate needs admin (no admin bypass at this
    level, unlike the project destroy) → 403."""
    member = seed.user()
    seed.member(tenant_a["workspace"]["id"], member["id"], role=15)
    member_headers = session_headers(
        seed, member, member["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Admin global 2"
    )
    r = api.delete(f"{_global_url(tenant_a)}{view_id}/", headers=member_headers)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_view_issues_shape(api, seed, tenant_a, project_a, auth_a):
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="Visible issue", description="plain text body",
    )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/issues/", headers=auth_a
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == PAGINATED_KEYS, sorted(set(body) ^ PAGINATED_KEYS)
    assert body["total_count"] == 1
    entry = body["results"][0]
    assert set(entry) == VIEW_ISSUE_KEYS, sorted(set(entry) ^ VIEW_ISSUE_KEYS)
    assert entry["id"] == issue_id
    assert entry["name"] == "Visible issue"
    assert entry["assignee_ids"] == []
    assert entry["label_ids"] == []
    assert entry["module_ids"] == []


def _utc():
    return datetime.now(timezone.utc)


def test_view_issues_m2m_ids_newest_first(
    api, seed, tenant_a, project_a, auth_a
):
    """``assignee_ids``/``label_ids``/``module_ids`` come back newest first.

    Django's ``IssueAssignee``/``IssueLabel``/``ModuleIssue`` all set
    ``Meta.ordering = ("-created_at",)``, so the prefetch SQL carries
    ``ORDER BY created_at DESC`` and the serializer's ``.all()`` iteration
    yields the ids newest-created first. Rust must match, so seed two of
    each with distinct ``created_at`` values and assert the order.
    """
    ws = tenant_a["workspace"]["id"]
    issue_id = seed.issue(ws, project_a, name="Ordered m2m links")
    older = _utc() - timedelta(hours=1)
    newer = _utc()

    assignee_a = seed.user()
    assignee_b = seed.user()
    seed.issue_assignee(ws, project_a, issue_id, assignee_a["id"], created_at=older)
    seed.issue_assignee(ws, project_a, issue_id, assignee_b["id"], created_at=newer)

    label_a = seed.label(ws, project_a)
    label_b = seed.label(ws, project_a)
    seed.issue_label(ws, project_a, issue_id, label_a, created_at=older)
    seed.issue_label(ws, project_a, issue_id, label_b, created_at=newer)

    module_a = seed.module(ws, project_a)
    module_b = seed.module(ws, project_a)
    seed.module_issue(ws, project_a, module_a, issue_id, created_at=older)
    seed.module_issue(ws, project_a, module_b, issue_id, created_at=newer)

    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/issues/", headers=auth_a
    )
    assert r.status_code == 200, r.text
    entry = next(e for e in r.json()["results"] if e["id"] == issue_id)
    assert entry["assignee_ids"] == [assignee_b["id"], assignee_a["id"]], entry
    assert entry["label_ids"] == [label_b, label_a], entry
    assert entry["module_ids"] == [module_b, module_a], entry


def test_view_issues_guest_sees_only_own(api, seed, tenant_a, project_a, auth_a):
    """Project guest without guest_view_all_features sees only issues they
    created."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=20)
    seed.project_member(project_a, tenant_a["workspace"]["id"], guest["id"], role=5)
    guest_headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    seed.issue(
        tenant_a["workspace"]["id"], project_a, name="Other owner issue",
    )
    guest_issue = seed.issue(
        tenant_a["workspace"]["id"], project_a, name="Guest issue",
    )
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (guest["id"], guest_issue),
        )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/issues/",
        headers=guest_headers,
    )
    assert r.status_code == 200, r.text
    assert [e["id"] for e in r.json()["results"]] == [guest_issue]


def test_view_issues_state_id_in_filter(api, seed, tenant_a, project_a, auth_a):
    # PIDASHCONV-1317: the global view-issues list runs the same
    # rich-filter compiler; UUID lists must bind typed (pre-fix Rust 500d).
    ws = tenant_a["workspace"]["id"]
    state_a = seed.state(project_a, ws, name="View state A", group="unstarted")
    state_b = seed.state(project_a, ws, name="View state B", group="started")
    issue_a = seed.issue(ws, project_a, name="View filter A", state_id=state_a)
    issue_b = seed.issue(ws, project_a, name="View filter B", state_id=state_b)
    url = f"/api/workspaces/{tenant_a['workspace']['slug']}/issues/"
    r = api.get(
        url, headers=auth_a,
        params={"filters": json.dumps({"and": [{"state_id__in": state_a}]})},
    )
    assert r.status_code == 200, r.text
    assert {e["id"] for e in r.json()["results"]} == {issue_a}
    r = api.get(
        url, headers=auth_a,
        params={"filters": json.dumps(
            {"and": [{"state_id__in": f"{state_a},{state_b}"}]})},
    )
    assert r.status_code == 200, r.text
    assert {e["id"] for e in r.json()["results"]} == {issue_a, issue_b}


def test_favorite_create_and_delete(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Star me",
    )
    base = (
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/user-favorite-views/"
    )
    r = api.post(base, headers=auth_a, json={"view": str(view_id)})
    assert r.status_code == 204, r.text
    r = api.delete(f"{base}{view_id}/", headers=auth_a)
    assert r.status_code == 204, r.text


def test_favorite_list_is_broken(api, seed, tenant_a, project_a, auth_a):
    """Port-existing-bug: GET favorites runs the default ModelViewSet.list,
    but IssueViewFavoriteViewSet declares no serializer_class (and its
    get_queryset references a nonexistent `view` relation), so it 500s.
    Pinned here so the Rust port reproduces the exact status + body."""
    base = (
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/user-favorite-views/"
    )
    r = api.get(base, headers=auth_a)
    assert r.status_code == 500, r.text
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_global_requires_auth(api, tenant_a):
    r = api.get(_global_url(tenant_a))
    assert r.status_code == 401, r.text


def test_global_tenant_isolation(api, seed, tenant_a, tenant_b, auth_b):
    """Tenant B (no membership in A's workspace) is denied at the
    WORKSPACE role gate — A's global views do not leak."""
    seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="A global"
    )
    r = api.get(_global_url(tenant_a), headers=auth_b)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}
