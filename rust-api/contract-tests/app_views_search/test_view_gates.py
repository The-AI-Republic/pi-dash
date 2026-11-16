# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""View permission gates (NEWPIDASH-202, DIFF-17).

Old bug fix: three view methods carry no permission gate at all on both
backends, so any authenticated user — including one who is not a member of
the workspace — can write into and read from workspaces and private
projects they do not belong to:

* ``POST /api/workspaces/<slug>/projects/<project_id>/views/`` — Django
  ``IssueViewViewSet`` defines ``perform_create`` only; the DRF default
  ``create`` runs with no ``@allow_permission``.
* ``POST /api/workspaces/<slug>/views/`` — same for ``WorkspaceViewViewSet``.
* ``PUT /api/workspaces/<slug>/views/<pk>/`` and
  ``PUT /api/workspaces/<slug>/projects/<project_id>/views/<pk>/`` — Django
  routes ``put`` to the DRF default ``update`` with no decorator, no owner
  check and no ``is_locked`` check (``partial_update`` has all three).
* ``GET /api/workspaces/<slug>/views/<pk>/`` — ``WorkspaceViewViewSet.retrieve``
  is undecorated (authN only); its queryset ``Q(owned_by=user) | Q(access=1)``
  lets any public view of any workspace through.

The new system deliberately differs (DIFF-17): the gates mirror the
neighbours Dart-side (``list``/``partial_update``/``destroy``):

* workspace create → WORKSPACE ``[ADMIN, MEMBER]`` (guests are not
  offered creation, VIEW-034);
* project create → PROJECT ``[ADMIN, MEMBER, GUEST]`` (guests may create,
  VIEW-003/VIEW-009);
* PUT → same creator / locked / owner chain as ``PATCH``;
* workspace retrieve → WORKSPACE ``[ADMIN, MEMBER, GUEST]``.

Members keep 201/200; non-members and outsiders go from 201/200 to 403
(and PUT non-owners from 200 to 403/400).  Old clients only create as
members and never send PUT, so no response they depend on changes.

Harness: ``expect("DIFF-17", django=..., rust=...)`` branches on
``PIDASH_CONTRACT_BACKEND`` (``django`` default, ``rust`` for the Rust
run).  Every case below pins the real old behaviour on Django (201/200)
and the fixed behaviour on Rust (403/400).
"""

from _harness.diffs import expect, is_rust

FORBIDDEN = {"error": "You don't have the required permissions."}
LOCKED = {"error": "view is locked"}
ONLY_OWNER = {"error": "Only the owner of the view can update the view"}


def _project_views_url(tenant, project_id):
    return f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/views/"


def _project_view_url(tenant, project_id, view_id):
    return f"{_project_views_url(tenant, project_id)}{view_id}/"


def _ws_views_url(tenant):
    return f"/api/workspaces/{tenant['workspace']['slug']}/views/"


def _ws_view_url(tenant, view_id):
    return f"{_ws_views_url(tenant)}{view_id}/"


def _headers_for(seed, user):
    import os

    from .conftest import session_headers

    return session_headers(seed, user, user["password"], os.environ["CONTRACT_SECRET_KEY"])


def _count_views(db, workspace_id, project_id=None, name=None):
    with db.cursor() as cur:
        if project_id is None:
            cur.execute(
                "SELECT count(*) FROM issue_views WHERE workspace_id = %s "
                "AND project_id IS NULL AND deleted_at IS NULL"
                + (" AND name = %s" if name else ""),
                (workspace_id, name) if name else (workspace_id,),
            )
        else:
            cur.execute(
                "SELECT count(*) FROM issue_views WHERE workspace_id = %s "
                "AND project_id = %s AND deleted_at IS NULL"
                + (" AND name = %s" if name else ""),
                (workspace_id, project_id, name) if name else (workspace_id, project_id),
            )
        return cur.fetchone()[0]


# ---------------------------------------------------------------------------
# (a) Outsider POSTs a project view into a private project → 403 / 201
# ---------------------------------------------------------------------------


def test_outsider_create_project_view_is_gated(api, seed, db, tenant_a, project_a):
    """(a) User with no workspace membership POSTs into private P2.

    Django: 201, the view is public and visible in the member list.
    Rust:   403, no row.
    """
    ws_id = tenant_a["workspace"]["id"]
    outsider = seed.user()
    headers = _headers_for(seed, outsider)
    name = "OUTSIDER-project-injected"
    before = _count_views(db, ws_id, project_id=project_a, name=name)
    res = api.post(
        _project_views_url(tenant_a, project_a),
        headers=headers,
        json={"name": name, "filters": {}},
    )
    assert res.status_code == expect("DIFF-17", django=201, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
        assert _count_views(db, ws_id, project_id=project_a, name=name) == before
    else:
        assert res.status_code == 201
        body = res.json()
        assert body["name"] == name
        assert body["project"] == project_a
        # Injected public project view shows up in the member's list (the
        # leak the issue reproduces).
        r = api.get(_project_views_url(tenant_a, project_a), headers=_headers_for(seed, tenant_a["user"]))
        assert r.status_code == 200, r.text
        assert any(v["name"] == name for v in r.json())
        # Cleanup the Django-created row (Rust created none).
        api.delete(_project_view_url(tenant_a, project_a, body["id"]), headers=_headers_for(seed, tenant_a["user"]))


def test_project_create_member_still_succeeds(api, seed, tenant_a, project_a, auth_a):
    """Regression: a project member keeps 201 on both backends."""
    res = api.post(
        _project_views_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "member-project-ok", "filters": {}},
    )
    assert res.status_code == 201, res.text
    vid = res.json()["id"]
    try:
        assert res.json()["project"] == project_a
    finally:
        api.delete(_project_view_url(tenant_a, project_a, vid), headers=auth_a)


def test_project_create_guest_still_succeeds(api, seed, tenant_a, project_a):
    """Project guests may create (VIEW-003/VIEW-009): 201 on both."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=20)
    seed.project_member(project_a, tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = _headers_for(seed, guest)
    res = api.post(
        _project_views_url(tenant_a, project_a),
        headers=headers,
        json={"name": "guest-project-ok", "filters": {}},
    )
    assert res.status_code == 201, res.text
    vid = res.json()["id"]
    try:
        assert res.json()["name"] == "guest-project-ok"
    finally:
        # Guest cannot delete another's view; the owner deletes.
        api.delete(_project_view_url(tenant_a, project_a, vid), headers=_headers_for(seed, tenant_a["user"]))


# ---------------------------------------------------------------------------
# (b) Outsider POSTs a workspace view → 403 / 201
# ---------------------------------------------------------------------------


def test_outsider_create_workspace_view_is_gated(api, seed, db, tenant_a):
    """(b) Same outsider POSTs a workspace (global) view.

    Django: 201.  Rust: 403.
    """
    ws_id = tenant_a["workspace"]["id"]
    outsider = seed.user()
    headers = _headers_for(seed, outsider)
    name = "OUTSIDER-ws-injected"
    before = _count_views(db, ws_id, project_id=None, name=name)
    res = api.post(
        _ws_views_url(tenant_a),
        headers=headers,
        json={"name": name, "filters": {}},
    )
    assert res.status_code == expect("DIFF-17", django=201, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
        assert _count_views(db, ws_id, project_id=None, name=name) == before
    else:
        assert res.status_code == 201
        body = res.json()
        assert body["name"] == name
        assert body["project"] is None
        r = api.get(_ws_views_url(tenant_a), headers=_headers_for(seed, tenant_a["user"]))
        assert r.status_code == 200, r.text
        assert any(v["name"] == name for v in r.json())
        api.delete(_ws_view_url(tenant_a, body["id"]), headers=_headers_for(seed, tenant_a["user"]))


def test_workspace_create_guest_denied_both(api, seed, tenant_a):
    """Workspace guests are not offered creation (VIEW-034): 403 on both
    (they were already membership-scoped out only on Rust; Django had no
    gate at all, so the Django arm is the fixed 403 only if we consider
    the suite's guest role — pin Rust 403, Django 201→403 divergence
    already covered by (b); here we pin the guest arm is also 403 on
    Rust and 201 on Django for completeness)."""
    # This case pins the same DIFF: guest hits workspace create.
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = _headers_for(seed, guest)
    name = "guest-ws-denied"
    res = api.post(_ws_views_url(tenant_a), headers=headers, json={"name": name, "filters": {}})
    assert res.status_code == expect("DIFF-17", django=201, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
    else:
        assert res.status_code == 201
        vid = res.json()["id"]
        api.delete(_ws_view_url(tenant_a, vid), headers=_headers_for(seed, tenant_a["user"]))


# ---------------------------------------------------------------------------
# (c) Non-owner member and guest PUT → 403/400 as PATCH, not 200
# ---------------------------------------------------------------------------


def test_put_nonowner_and_guest_workspace_view_is_gated(api, seed, tenant_a, auth_a):
    """(c) A workspace member who does not own the view and a workspace
    guest each PUT a new name onto the admin's workspace view.

    Django: 200, the row is renamed.
    Rust:   403 (creator gate; PATCH's chain), the row keeps its name.
    """
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Admin ws view"
    )
    url = _ws_view_url(tenant_a, view_id)

    # Non-owner MEMBER (role 15, workspace member, not creator)
    member = seed.user()
    seed.member(tenant_a["workspace"]["id"], member["id"], role=15)
    member_headers = _headers_for(seed, member)
    res = api.put(url, headers=member_headers, json={"name": "HACKED-member", "filters": {}})
    assert res.status_code == expect("DIFF-17", django=200, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
        # Row unchanged on Rust
        got = api.get(url, headers=auth_a)
        assert got.status_code == 200, got.text
        assert got.json()["name"] == "Admin ws view"
    else:
        assert res.status_code == 200
        assert res.json()["name"] == "HACKED-member"
        # Restore for the next step
        r = api.put(url, headers=auth_a, json={"name": "Admin ws view", "filters": {}})
        assert r.status_code == 200, r.text

    # Workspace GUEST (role 5, not creator)
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    guest_headers = _headers_for(seed, guest)
    res = api.put(url, headers=guest_headers, json={"name": "HACKED-guest", "filters": {}})
    assert res.status_code == expect("DIFF-17", django=200, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
        got = api.get(url, headers=auth_a)
        assert got.json()["name"] == "Admin ws view"
    else:
        assert res.status_code == 200
        assert res.json()["name"] == "HACKED-guest"
        r = api.put(url, headers=auth_a, json={"name": "Admin ws view", "filters": {}})
        assert r.status_code == 200, r.text


def test_put_locked_workspace_view_is_gated(api, seed, tenant_a, auth_a):
    """PUT on a locked view: Django 200 (bypasses the lock), Rust 400."""
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Locked ws", locked=True
    )
    url = _ws_view_url(tenant_a, view_id)
    res = api.put(url, headers=auth_a, json={"name": "Unlocked", "filters": {}})
    assert res.status_code == expect("DIFF-17", django=200, rust=400), res.text
    if is_rust():
        assert res.json() == LOCKED
        got = api.get(url, headers=auth_a)
        assert got.json()["name"] == "Locked ws"
        assert got.json()["is_locked"] is True
    else:
        assert res.status_code == 200


def test_put_nonowner_project_view_is_gated(api, seed, tenant_a, project_a, auth_a):
    """Same bypass on the project route: a non-owner project member
    (role 15, workspace 20) PUTs the admin's project view.

    Django: 200. Rust: 403 (creator gate) — the PATCH twin is 403 too
    for a non-admin non-creator; a workspace-admin fallback case is
    pinned separately below.
    """
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Admin proj view",
    )
    url = _project_view_url(tenant_a, project_a, view_id)

    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=15)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=15)
    headers = _headers_for(seed, other)
    res = api.put(url, headers=headers, json={"name": "HACKED-proj", "filters": {}})
    assert res.status_code == expect("DIFF-17", django=200, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
        got = api.get(url, headers=auth_a)
        assert got.json()["name"] == "Admin proj view"
    else:
        assert res.status_code == 200
        assert res.json()["name"] == "HACKED-proj"
        # Purge Django's renamed row so later tests see a clean slate
        r = api.put(url, headers=auth_a, json={"name": "Admin proj view", "filters": {}})
        assert r.status_code == 200, r.text


def test_put_owner_still_succeeds(api, seed, tenant_a, auth_a):
    """Regression: owner PUT still 200 on both."""
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Owner before"
    )
    url = _ws_view_url(tenant_a, view_id)
    res = api.put(url, headers=auth_a, json={"name": "Owner after", "filters": {}})
    assert res.status_code == 200, res.text
    assert res.json()["name"] == "Owner after"
    # Workspace PUT has no is_favorite (un-annotated queryset)
    assert "is_favorite" not in res.json()


def test_put_project_owner_still_succeeds(api, seed, tenant_a, project_a, auth_a):
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="Proj before",
    )
    url = _project_view_url(tenant_a, project_a, view_id)
    res = api.put(url, headers=auth_a, json={"name": "Proj after", "filters": {}})
    assert res.status_code == 200, res.text
    assert res.json()["name"] == "Proj after"
    # Project PUT goes through the annotated queryset
    assert res.json()["is_favorite"] is False


# ---------------------------------------------------------------------------
# (d) Outsider GETs a workspace view by id → 403 / 200
# ---------------------------------------------------------------------------


def test_outsider_retrieve_workspace_view_is_gated(api, seed, tenant_a, auth_a):
    """(d) Outsider (no workspace membership) GETs the admin's public
    workspace view by id.

    Django: 200 with the full definition (queryset lets any public view
    through). Rust: 403 (WORKSPACE gate).
    """
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Public ws view"
    )
    url = _ws_view_url(tenant_a, view_id)
    outsider = seed.user()
    headers = _headers_for(seed, outsider)
    res = api.get(url, headers=headers)
    assert res.status_code == expect("DIFF-17", django=200, rust=403), res.text
    if is_rust():
        assert res.json() == FORBIDDEN
    else:
        assert res.status_code == 200
        body = res.json()
        assert body["id"] == view_id
        assert body["name"] == "Public ws view"
        assert body["project"] is None


def test_workspace_retrieve_member_still_succeeds(api, seed, tenant_a, auth_a):
    """Regression: a workspace member retrieves on both."""
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="Member-read"
    )
    res = api.get(_ws_view_url(tenant_a, view_id), headers=auth_a)
    assert res.status_code == 200, res.text
    assert res.json()["name"] == "Member-read"


def test_workspace_retrieve_guest_scoped_still_succeeds(api, seed, tenant_a):
    """Workspace guest may retrieve its own view (list narrowing already
    scopes guests to owned rows; retrieve now gates to ADMIN/MEMBER/GUEST
    so a guest still passes the gate — the queryset filter still scopes
    to owned-or-public, matching the old member-only behaviour)."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = _headers_for(seed, guest)
    view_id = seed.issue_view(
        tenant_a["workspace"]["id"], guest["id"], name="Guest own"
    )
    res = api.get(_ws_view_url(tenant_a, view_id), headers=headers)
    assert res.status_code == 200, res.text
    assert res.json()["name"] == "Guest own"
