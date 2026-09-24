"""Stickies viewset: create/list/retrieve/partial-update/destroy.

``StickyViewSet`` (``/api/v1/workspaces/<slug>/stickies/`` + ``<pk>/``) is
gated by ``WorkspaceUserPermission`` — any active workspace member (admin,
member or guest) may use every action. Rows are additionally owner-scoped:
``get_queryset`` filters ``owner=request.user``, so a member only ever sees
their own stickies. List is ``-created_at`` ordered with ``?query=`` matched
against ``description_stripped``.
"""

STICKY_KEYS = {
    "background_color", "color", "created_at", "created_by", "deleted_at",
    "description", "description_binary", "description_html",
    "description_stripped", "id", "logo_props", "name", "owner",
    "sort_order", "updated_at", "updated_by", "workspace",
}

LIST_KEYS = {
    "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
    "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
    "total_count", "total_pages", "total_results",
}


def _base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def _create(org, role="admin", **overrides):
    payload = {"name": "note", "description_html": "<p>hello world</p>", **overrides}
    r = org.request("POST", _base(org), role, json=payload)
    assert r.status_code == 201, r.text[:500]
    return r.json()


def test_sticky_create_shape(org):
    body = _create(org)
    assert set(body) == STICKY_KEYS
    assert body["name"] == "note"
    assert body["description_html"] == "<p>hello world</p>"
    assert body["description_stripped"] == "hello world"
    assert body["workspace"] == org.workspace["id"]
    assert body["owner"] == org.admin["id"]
    assert body["sort_order"] == 65535.0


def test_sticky_list_shape_and_order(org):
    first = _create(org, name="first")
    second = _create(org, name="second")
    r = org.request("GET", _base(org), "admin")
    assert r.status_code == 200
    body = r.json()
    assert set(body) == LIST_KEYS
    assert body["total_count"] == 2
    assert [row["id"] for row in body["results"]] == [second["id"], first["id"]]
    for row in body["results"]:
        assert set(row) == STICKY_KEYS


def test_sticky_list_query_filters_description(org):
    _create(org, name="alpha", description_html="<p>pineapple notes</p>")
    _create(org, name="beta", description_html="<p>unrelated</p>")
    r = org.request("GET", _base(org), "admin", params={"query": "pineapple"})
    assert r.status_code == 200
    results = r.json()["results"]
    assert [row["name"] for row in results] == ["alpha"]


def test_sticky_retrieve_shape(org):
    created = _create(org)
    r = org.request("GET", f"{_base(org)}{created['id']}/", "admin")
    assert r.status_code == 200
    assert r.json() == created


def test_sticky_partial_update(org):
    created = _create(org)
    r = org.request(
        "PATCH", f"{_base(org)}{created['id']}/", "admin",
        json={"name": "renamed", "color": "#ff0000"},
    )
    assert r.status_code == 200
    body = r.json()
    assert set(body) == STICKY_KEYS
    assert body["name"] == "renamed"
    assert body["color"] == "#ff0000"
    assert body["description_html"] == "<p>hello world</p>"


def test_sticky_delete(org):
    created = _create(org)
    r = org.request("DELETE", f"{_base(org)}{created['id']}/", "admin")
    assert r.status_code == 204
    assert r.text == ""
    r = org.request("GET", f"{_base(org)}{created['id']}/", "admin")
    assert r.status_code == 404


def test_sticky_list_owner_scoped(org):
    _create(org, role="admin", name="admin-note")
    r = org.request("GET", _base(org), "member")
    assert r.status_code == 200
    assert r.json()["results"] == []
    mine = _create(org, role="member", name="member-note")
    r = org.request("GET", _base(org), "member")
    assert [row["id"] for row in r.json()["results"]] == [mine["id"]]


def test_sticky_retrieve_other_owners_404(org):
    created = _create(org, role="admin")
    r = org.request("GET", f"{_base(org)}{created['id']}/", "member")
    assert r.status_code == 404


def test_sticky_guest_may_create(org):
    body = _create(org, role="guest", name="guest-note")
    assert body["owner"] == org.guest["id"]
