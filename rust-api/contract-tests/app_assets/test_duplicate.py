"""Asset duplication: validation, gates, throttle, and the partial-write quirk.

``DuplicateAssetEndpoint.post`` creates the duplicated row *before* copying
the bytes in object storage, so when the copy fails the API answers 500
while the row stays behind with ``is_uploaded=True``. The success body
therefore depends on live object storage; the suite pins the deterministic
parts (validation, permission, throttle, and the row the view always
creates) instead of the storage-mediated status.
"""

from . import seed_assets as seed_a


def dup_path(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/duplicate-assets/{asset_id}/"
    )


def dup_payload(org, **kw):
    # Note the key name: duplicate reads "entity_id", unlike the mint
    # endpoints which read "entity_identifier" (pinned quirk below).
    body = {"entity_type": "WORKSPACE_LOGO",
            "entity_id": org.workspace["id"]}
    body.update(kw)
    return body


def mint_uploaded(org, name="orig.png"):
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", "admin",
        json={"name": name, "type": "image/png", "size": 120,
              "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    )
    asset_id = r.json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    return asset_id


def test_duplicate_invalid_entity_type_400(org):
    asset_id = mint_uploaded(org)
    r = org.request(
        "POST", dup_path(org, asset_id), "admin",
        json={"entity_type": "NOPE"},
    )
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type or entity id"}


def test_duplicate_unknown_asset_404(org):
    r = org.request(
        "POST",
        dup_path(org, "00000000-0000-0000-0000-000000000000"),
        "admin",
        json=dup_payload(org),
    )
    assert r.status_code == 404
    assert r.json() == {"error": "Asset not found"}


def test_duplicate_denied_roles(org):
    asset_id = mint_uploaded(org)
    denied = {"error": "You don't have the required permissions."}
    r = org.request(
        "POST", dup_path(org, asset_id), "outsider", json=dup_payload(org)
    )
    assert r.status_code == 403
    assert r.json() == denied
    r = org.request("POST", dup_path(org, asset_id), None, json=dup_payload(org))
    assert r.status_code == 401


def test_duplicate_always_creates_the_row(org):
    # The duplicated row is inserted before the storage copy, so it exists
    # whether the copy succeeds (200 {"asset_id"}) or fails (500).
    asset_id = mint_uploaded(org, name="copyme.png")
    before = {
        row[0]
        for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
    }
    r = org.request(
        "POST", dup_path(org, asset_id), "admin", json=dup_payload(org)
    )
    assert r.status_code in (200, 500), r.text[:300]
    if r.status_code == 200:
        assert set(r.json().keys()) == {"asset_id"}
        new_id = r.json()["asset_id"]
    else:
        assert r.json() == {"error": "Something went wrong please try again later"}
        after = {
            row[0]
            for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
        }
        assert len(after - before) == 1
        new_id = next(iter(after - before))
    original = seed_a.fetch(org.conn, asset_id)
    clone = seed_a.fetch(org.conn, str(new_id))
    assert clone["attributes"] == original["attributes"]
    assert clone["size"] == original["size"]
    assert clone["workspace_id"] == original["workspace_id"]
    # is_uploaded flips only after a successful copy, so it tracks the
    # storage outcome instead of being pinned to either value.
    assert clone["is_uploaded"] is (r.status_code == 200)
    assert clone["asset"] != original["asset"]
    assert clone["asset"].endswith("-copyme.png")


def test_duplicate_ignores_entity_identifier(org):
    # Ported quirk: "entity_identifier" (the mint-endpoint spelling) is
    # ignored here, so the clone lands with a NULL workspace.
    asset_id = mint_uploaded(org, name="quirk.png")
    before = {
        row[0]
        for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
    }
    r = org.request(
        "POST", dup_path(org, asset_id), "admin",
        json={"entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    )
    assert r.status_code in (200, 500), r.text[:300]
    after = {
        row[0]
        for row in org.conn.execute("SELECT id FROM file_assets").fetchall()
    }
    new_id = (r.json()["asset_id"] if r.status_code == 200
              else next(iter(after - before)))
    assert seed_a.fetch(org.conn, str(new_id))["workspace_id"] is None


def test_duplicate_throttle(org):
    # AssetRateThrottle allows 5/minute per asset_id; the 6th request trips
    # it. A not-yet-uploaded source answers 404 (fast, no storage touch), so
    # all six requests run the same gate path and only the throttle differs.
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", "admin",
        json={"name": "t.png", "type": "image/png", "size": 10,
              "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    )
    asset_id = r.json()["asset_id"]
    codes = [
        org.request(
            "POST", dup_path(org, asset_id), "admin", json=dup_payload(org)
        ).status_code
        for _ in range(6)
    ]
    assert codes[:5] == [404] * 5
    assert codes[5] == 429
    r = org.request(
        "POST", dup_path(org, asset_id), "admin", json=dup_payload(org)
    )
    assert r.status_code == 429
    # Throttled answers use the product rate-limit envelope, not DRF detail.
    assert r.json() == {"error_code": 5900, "error_message": "RATE_LIMIT_EXCEEDED"}
