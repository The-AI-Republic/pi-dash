"""Tenant-isolation contract: what crosses a workspace/user boundary.

Two groups of pins. Gated routes (project, bulk, check, restore, duplicate,
downloads) deny cross-tenant callers with 403 — including an admin of an
unrelated workspace. Ungated routes (workspace v2 mint/fetch/confirm/delete)
enforce only authentication: any signed-in user may act on any workspace's
rows. That gap is the shipped Django behavior and is pinned here so the
Rust port reproduces it instead of silently "fixing" it.
"""

from . import seed_assets as seed_a
from .conftest import make_world

DENIED = {"error": "You don't have the required permissions."}


def ws_post(org, role="admin", **kw):
    payload = {
        "name": "gap.png",
        "type": "image/png",
        "size": 100,
        "entity_type": "WORKSPACE_LOGO",
        "entity_identifier": org.workspace["id"],
    }
    payload.update(kw)
    return org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", role, json=payload
    )


def gated_paths(org, asset_id):
    slug = org.workspace["slug"]
    pid = org.project["id"]
    return {
        "proj-post": (
            "POST",
            f"/api/assets/v2/workspaces/{slug}/projects/{pid}/",
        ),
        "proj-get": (
            "GET",
            f"/api/assets/v2/workspaces/{slug}/projects/{pid}/{asset_id}/",
        ),
        "check": ("GET", f"/api/assets/v2/workspaces/{slug}/check/{asset_id}/"),
        "restore": (
            "POST",
            f"/api/assets/v2/workspaces/{slug}/restore/{asset_id}/",
        ),
        "duplicate": (
            "POST",
            f"/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/",
        ),
        "ws-download": (
            "GET",
            f"/api/assets/v2/workspaces/{slug}/download/{asset_id}/",
        ),
        "proj-download": (
            "GET",
            f"/api/assets/v2/workspaces/{slug}/projects/{pid}/download/{asset_id}/",
        ),
    }


def test_admin_of_other_workspace_denied_on_gated(org, pg):
    other = make_world(pg)
    try:
        asset_id = ws_post(org).json()["asset_id"]
        seed_a.mark_uploaded(org.conn, asset_id)
        for name, (method, path) in gated_paths(org, asset_id).items():
            # Every gate runs before object lookup/validation, so any JSON
            # body reaches the 403 first.
            payload = (
                {"name": "x.png", "entity_type": "ISSUE_DESCRIPTION"}
                if method == "POST" else {}
            )
            r = other.request(method, path, "admin", json=payload)
            assert r.status_code == 403, name
            assert r.json() == DENIED, name
    finally:
        other.close()


def test_outsider_may_mint_in_foreign_workspace(org):
    # No membership gate on the workspace mint: documents the gap.
    r = ws_post(org, "outsider")
    assert r.status_code == 200, r.text
    row = seed_a.fetch(org.conn, r.json()["asset_id"])
    assert row["workspace_id"] == org.workspace["id"]


def test_outsider_may_read_confirm_delete_foreign_asset(org):
    asset_id = ws_post(org).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/{asset_id}/",
        "outsider",
    )
    assert r.status_code == 302
    r = org.request(
        "PATCH",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/{asset_id}/",
        "outsider",
        json={},
    )
    assert r.status_code == 204
    r = org.request(
        "DELETE",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/{asset_id}/",
        "outsider",
    )
    assert r.status_code == 204


def test_user_assets_scoped_to_creator(org):
    r = org.request(
        "POST", "/api/assets/v2/user-assets/", "admin",
        json={"name": "a.png", "type": "image/png", "size": 10,
              "entity_type": "USER_AVATAR"},
    )
    asset_id = r.json()["asset_id"]
    for method in ("PATCH", "DELETE"):
        r = org.request(
            method, f"/api/assets/v2/user-assets/{asset_id}/", "outsider",
            json={},
        )
        assert r.status_code == 404, method
        assert r.json() == {"error": "The required object does not exist."}, method


def test_cross_workspace_slug_mismatch_404(org, pg):
    other = make_world(pg)
    try:
        asset_id = ws_post(org).json()["asset_id"]
        seed_a.mark_uploaded(org.conn, asset_id)
        r = other.request(
            "GET",
            f"/api/assets/v2/workspaces/{other.workspace['slug']}/{asset_id}/",
            "admin",
        )
        assert r.status_code == 404
        assert r.json() == {"error": "The required object does not exist."}
    finally:
        other.close()
