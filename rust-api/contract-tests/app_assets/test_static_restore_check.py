"""Static serving, restore, and existence check (v2).

Static is the only ``AllowAny`` route in the domain. Restore is idempotent.
Check answers from ``all_objects`` plus ``deleted_at``, so v1-style deletes
(which set only ``is_deleted``) still report ``exists: true``.
"""

from . import seed_assets as seed_a


def ws_post(org, role="admin", **kw):
    payload = {
        "name": "logo.png",
        "type": "image/png",
        "size": 100,
        "entity_type": "WORKSPACE_LOGO",
        "entity_identifier": org.workspace["id"],
    }
    payload.update(kw)
    return org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", role, json=payload
    )


def check(org, asset_id, role="admin"):
    return org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/check/{asset_id}/",
        role,
    )


def test_static_anonymous_redirect(org):
    asset_id = ws_post(org).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.client(None).get(f"/api/assets/v2/static/{asset_id}/")
    assert r.status_code == 302, r.text
    location = r.headers["location"]
    row = seed_a.fetch(org.conn, asset_id)
    assert row["asset"] in location
    assert "response-content-disposition=inline" in location


def test_static_before_upload_404(org):
    asset_id = ws_post(org).json()["asset_id"]
    r = org.request("GET", f"/api/assets/v2/static/{asset_id}/", "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}


def test_static_unknown_asset_404(org):
    r = org.request(
        "GET", "/api/assets/v2/static/00000000-0000-0000-0000-000000000000/",
        "admin",
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_static_disallowed_entity_type_400(org):
    asset = seed_a.create_asset(
        org.conn,
        workspace_id=org.workspace["id"],
        created_by_id=org.admin["id"],
        name="note.png",
        entity_type="ISSUE_ATTACHMENT",
        project_id=org.project["id"],
        uploaded=True,
    )
    r = org.request("GET", f"/api/assets/v2/static/{asset['id']}/", "admin")
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_restore_round_trip(org):
    asset_id = ws_post(org).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    org.request(
        "DELETE",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/{asset_id}/",
        "admin",
    )
    assert check(org, asset_id).json() == {"exists": False}
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/restore/{asset_id}/",
        "admin",
    )
    assert r.status_code == 204
    assert r.text == ""
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_deleted"] is False
    assert row["deleted_at"] is None
    assert check(org, asset_id).json() == {"exists": True}


def test_restore_live_asset_is_idempotent(org):
    asset_id = ws_post(org).json()["asset_id"]
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/restore/{asset_id}/",
        "admin",
    )
    assert r.status_code == 204


def test_restore_unknown_asset_404(org):
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        "/restore/00000000-0000-0000-0000-000000000000/",
        "admin",
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_restore_denied_roles(org):
    asset_id = ws_post(org).json()["asset_id"]
    denied = {"error": "You don't have the required permissions."}
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/restore/{asset_id}/",
        "outsider",
    )
    assert r.status_code == 403
    assert r.json() == denied
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/restore/{asset_id}/",
        None,
    )
    assert r.status_code == 401


def test_check_true_false_unknown(org):
    asset_id = ws_post(org).json()["asset_id"]
    assert check(org, asset_id).json() == {"exists": True}
    assert check(
        org, "00000000-0000-0000-0000-000000000000"
    ).json() == {"exists": False}


def test_check_denied_roles(org):
    asset_id = ws_post(org).json()["asset_id"]
    denied = {"error": "You don't have the required permissions."}
    r = check(org, asset_id, "outsider")
    assert r.status_code == 403
    assert r.json() == denied
    r = check(org, asset_id, None)
    assert r.status_code == 401


def test_check_guest_allowed(org):
    asset_id = ws_post(org).json()["asset_id"]
    assert check(org, asset_id, "guest").json() == {"exists": True}
