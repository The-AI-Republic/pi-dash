"""Project assets and bulk attach (v2).

``ProjectAssetEndpoint`` is PROJECT-level gated (workspace admins in the
project plus project members incl. guests). ``ProjectBulkAssetEndpoint``
links already-uploaded rows to an entity. ``entity_id`` in the bulk path is
a ``<uuid:>`` converter — non-UUID values never reach the view.
"""

import uuid

from . import seed_assets as seed_a


def proj_base(org):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/"
    )


def proj_detail(org, asset_id):
    return f"{proj_base(org)}{asset_id}/"


def proj_payload(**kw):
    body = {
        "name": "desc.png",
        "type": "image/png",
        "size": 200,
        "entity_type": "ISSUE_DESCRIPTION",
        "entity_identifier": None,
    }
    body.update(kw)
    return body


def test_post_shape_and_asset_url(org):
    r = org.request("POST", proj_base(org), "admin", json=proj_payload())
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"upload_data", "asset_id", "asset_url"}
    assert set(body["upload_data"].keys()) == {"url", "fields"}
    assert body["upload_data"]["fields"]["key"].endswith("-desc.png")
    assert body["asset_url"] == (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/{body['asset_id']}/"
    )
    row = seed_a.fetch(org.conn, body["asset_id"])
    assert row["workspace_id"] == org.workspace["id"]
    assert row["project_id"] == org.project["id"]
    assert row["entity_type"] == "ISSUE_DESCRIPTION"


def test_post_project_cover_500_duplicate_kwarg(org):
    # Ported bug: PROJECT_COVER resolves to {"project_id": ...} which
    # collides with the explicit project_id kwarg → TypeError → 500.
    r = org.request(
        "POST", proj_base(org), "admin",
        json=proj_payload(entity_type="PROJECT_COVER",
                          entity_identifier=org.project["id"]),
    )
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_post_invalid_entity_type(org):
    r = org.request(
        "POST", proj_base(org), "admin", json=proj_payload(entity_type="NOPE")
    )
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_post_roles(org):
    denied = {"error": "You don't have the required permissions."}
    # Guests of the project may mint uploads …
    r = org.request("POST", proj_base(org), "guest", json=proj_payload(name="g.png"))
    assert r.status_code == 200, r.text
    # … but a workspace member outside the project may not …
    outsider_payload = proj_payload(name="o.png")
    r = org.request("POST", proj_base(org), "outsider", json=outsider_payload)
    assert r.status_code == 403
    assert r.json() == denied
    # … and anonymous callers get 401.
    r = org.request("POST", proj_base(org), None, json=proj_payload(name="n.png"))
    assert r.status_code == 401


def test_workspace_member_without_project_role_denied(org):
    from _harness import seed, sessions
    from _harness import http as http_h

    user = seed.create_user(
        org.conn,
        email=f"wmonly-{org.tag}@ct.example.com",
        username=f"wmonly-{org.tag}",
        password_field=sessions.make_password_hash(f"pw-{org.tag}"),
    )
    seed.add_workspace_member(
        org.conn, workspace_id=org.workspace["id"], user_id=user["id"],
        role=seed.MEMBER,
    )
    cookies = sessions.login(org.conn, user["id"], user["password_field"])
    client = http_h.make_client(cookies)
    try:
        r = client.post(proj_base(org), json=proj_payload(name="w.png"))
        assert r.status_code == 403
        assert r.json() == {"error": "You don't have the required permissions."}
        # The same caller passes WORKSPACE-level gates (check) …
        asset_id = org.request(
            "POST",
            f"/api/assets/v2/workspaces/{org.workspace['slug']}/",
            "admin",
            json={"name": "w.png", "type": "image/png", "size": 10,
                  "entity_type": "WORKSPACE_LOGO",
                  "entity_identifier": org.workspace["id"]},
        ).json()["asset_id"]
        r = client.get(
            f"/api/assets/v2/workspaces/{org.workspace['slug']}/check/{asset_id}/"
        )
        assert r.status_code == 200
        assert r.json() == {"exists": True}
    finally:
        client.close()


def test_patch_get_delete_round_trip(org):
    asset_id = org.request("POST", proj_base(org), "admin", json=proj_payload()).json()["asset_id"]
    r = org.request("GET", proj_detail(org, asset_id), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}
    r = org.request("PATCH", proj_detail(org, asset_id), "admin", json={})
    assert r.status_code == 204
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_uploaded"] is True
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("GET", proj_detail(org, asset_id), "admin")
    assert r.status_code == 302
    assert seed_a.fetch(org.conn, asset_id)["asset"] in r.headers["location"]
    r = org.request("DELETE", proj_detail(org, asset_id), "admin")
    assert r.status_code == 204
    assert seed_a.fetch(org.conn, asset_id)["is_deleted"] is True


def test_detail_denied_roles(org):
    asset_id = org.request("POST", proj_base(org), "admin", json=proj_payload()).json()["asset_id"]
    denied = {"error": "You don't have the required permissions."}
    for method in ("GET", "PATCH", "DELETE"):
        r = org.request(method, proj_detail(org, asset_id), "outsider", json={})
        assert r.status_code == 403, method
        assert r.json() == denied, method
    r = org.request("GET", proj_detail(org, asset_id), None)
    assert r.status_code == 401


def _cover_asset(org, name):
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", "admin",
        json={"name": name, "type": "image/png", "size": 60,
              "entity_type": "PROJECT_COVER",
              "entity_identifier": org.project["id"]},
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def bulk_path(org, entity_id):
    return f"{proj_base(org)}{entity_id}/bulk/"


def test_bulk_cover_links_project(org):
    first = _cover_asset(org, "c1.png")
    second = _cover_asset(org, "c2.png")
    entity_id = str(uuid.uuid4())
    r = org.request(
        "POST", bulk_path(org, entity_id), "admin",
        json={"asset_ids": [first, second]},
    )
    assert r.status_code == 204, r.text
    for aid in (first, second):
        assert seed_a.fetch(org.conn, aid)["project_id"] == org.project["id"]
    cover = org.conn.execute(
        "SELECT cover_image_asset_id FROM projects WHERE id = %s",
        (org.project["id"],),
    ).fetchone()[0]
    assert str(cover) in (first, second)


def test_bulk_empty_400(org):
    r = org.request(
        "POST", bulk_path(org, str(uuid.uuid4())), "admin", json={"asset_ids": []}
    )
    assert r.status_code == 400
    assert r.json() == {"error": "No asset ids provided."}


def test_bulk_unknown_ids_404(org):
    r = org.request(
        "POST", bulk_path(org, str(uuid.uuid4())), "admin",
        json={"asset_ids": ["00000000-0000-0000-0000-000000000000"]},
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}


def test_bulk_roles(org):
    first = _cover_asset(org, "c3.png")
    denied = {"error": "You don't have the required permissions."}
    r = org.request(
        "POST", bulk_path(org, str(uuid.uuid4())), "outsider",
        json={"asset_ids": [first]},
    )
    assert r.status_code == 403
    assert r.json() == denied
    r = org.request(
        "POST", bulk_path(org, str(uuid.uuid4())), "guest",
        json={"asset_ids": [first]},
    )
    assert r.status_code == 204
    r = org.request(
        "POST", bulk_path(org, str(uuid.uuid4())), None,
        json={"asset_ids": [first]},
    )
    assert r.status_code == 401
