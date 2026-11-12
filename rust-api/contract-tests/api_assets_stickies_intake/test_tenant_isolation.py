"""Tenant-isolation cases: one per surface.

An authenticated caller from outside the tenant boundary must never observe
or mutate another tenant's rows. Depending on where the scoping happens the
backend answers 403 (permission gate: stickies, intake) or 404 (row lookup
scoped by user/workspace: user assets, generic assets).
"""

import uuid


def test_sticky_cross_workspace_admin_403(org):
    r = org.request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}/stickies/",
        "other_admin",
    )
    assert r.status_code == 403


def test_sticky_cross_workspace_create_403(org):
    r = org.request(
        "POST",
        f"/api/v1/workspaces/{org.workspace['slug']}/stickies/",
        "other_admin",
        json={"name": "invasion"},
    )
    assert r.status_code == 403
    assert org.conn.execute("SELECT count(*) FROM stickies WHERE name = 'invasion'").fetchone()[0] == 0


def test_intake_cross_workspace_admin_403(org):
    base = (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )
    r = org.request("GET", base, "other_admin")
    assert r.status_code == 403
    r = org.request("POST", base, "other_admin", json={"issue": {"name": "invasion"}})
    assert r.status_code == 403


def test_intake_cross_project_member_403(org):
    # ``member`` belongs to project 1 but not project 2 (same workspace).
    base = (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project2['id']}/intake-issues/"
    )
    r = org.request("GET", base, "member")
    assert r.status_code == 403
    r = org.request("POST", base, "member", json={"issue": {"name": "invasion"}})
    assert r.status_code == 403
    # The project admin is unaffected.
    created = org.conn.execute(
        "SELECT count(*) FROM intake_issues WHERE project_id = %s", (org.project2["id"],)
    ).fetchone()[0]
    assert created == 0
    r = org.request("GET", base, "admin")
    assert r.status_code == 200


def test_intake_guest_cross_project_403(org):
    base = (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project2['id']}/intake-issues/"
    )
    r = org.request("GET", base, "guest")
    assert r.status_code == 403


def test_user_asset_cross_user_invisible(org):
    r = org.request(
        "POST", "/api/v1/assets/user-assets/", "admin",
        json={"name": "mine.jpg", "type": "image/jpeg", "size": 10, "entity_type": "USER_AVATAR"},
    )
    asset_id = r.json()["asset_id"]
    for method, path, kw in (
        ("PATCH", f"/api/v1/assets/user-assets/{asset_id}/", {"json": {}}),
        ("DELETE", f"/api/v1/assets/user-assets/{asset_id}/", {}),
    ):
        r = org.request(method, path, "other_admin", **kw)
        assert r.status_code == 404, (method, r.status_code)


def test_generic_asset_cross_workspace_invisible(org):
    r = org.request(
        "POST", f"/api/v1/workspaces/{org.workspace['slug']}/assets/", "admin",
        json={"name": "w1.pdf", "type": "application/pdf", "size": 10},
    )
    asset_id = r.json()["asset_id"]
    org.conn.execute("UPDATE file_assets SET is_uploaded = true WHERE id = %s", (asset_id,))
    # Same asset id under the other workspace slug does not resolve.
    r = org.request(
        "GET",
        f"/api/v1/workspaces/{org.workspace2['slug']}/assets/{asset_id}/",
        "other_admin",
    )
    assert r.status_code == 404
    # Nor does a random id under the home workspace.
    r = org.request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}/assets/{uuid.uuid4()}/",
        "admin",
    )
    assert r.status_code == 404
