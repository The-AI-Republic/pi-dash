# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""JSON "" on relational FKs clears like DRF (PIDASHCONV-1127).

``RelatedField.run_validation`` forces the exact empty string to ``None``
on every path, JSON included, so nullable FKs clear instead of 400ing.
``updated_by`` is the exception that proves the rule: ``BaseModel.save``
unconditionally overwrites it with the request user on update, so even an
explicit UUID (or "") stores the actor, never the input. Whitespace is not
empty: " " still fails as an invalid UUID on both backends.
"""

LCURLY = "“"
RCURLY = "”"


def _instance_audit_row(db):
    with db.connect() as conn:
        return conn.execute(
            "select created_by_id, updated_by_id from instances;"
        ).fetchone()


def test_patch_created_by_empty_string_clears(admin_api, world):
    with world["db"].connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE instances SET created_by_id = %s;",
                (world["member"]["id"],),
            )
        conn.commit()
    res = admin_api.patch("/api/instances/", json={"created_by": ""})
    assert res.status_code == 200
    assert res.json()["created_by"] is None
    created_by, _ = _instance_audit_row(world["db"])
    assert created_by is None


def test_patch_updated_by_empty_string_stamps_actor(admin_api, world):
    res = admin_api.patch("/api/instances/", json={"updated_by": ""})
    assert res.status_code == 200
    assert res.json()["updated_by"] == world["admin"]["id"]
    _, updated_by = _instance_audit_row(world["db"])
    assert str(updated_by) == world["admin"]["id"]


def test_patch_updated_by_explicit_uuid_stamps_actor(admin_api, world):
    # BaseModel.save overwrites updated_by with the request user: the
    # explicit value is validated (bad UUIDs still 400) but never stored.
    res = admin_api.patch(
        "/api/instances/", json={"updated_by": world["member"]["id"]}
    )
    assert res.status_code == 200
    assert res.json()["updated_by"] == world["admin"]["id"]
    _, updated_by = _instance_audit_row(world["db"])
    assert str(updated_by) == world["admin"]["id"]


def test_patch_created_by_explicit_uuid_sets(admin_api, world):
    # Control: created_by has no save() overwrite, so valid input stores.
    res = admin_api.patch(
        "/api/instances/", json={"created_by": world["member"]["id"]}
    )
    assert res.status_code == 200
    assert res.json()["created_by"] == world["member"]["id"]
    created_by, _ = _instance_audit_row(world["db"])
    assert str(created_by) == world["member"]["id"]


def test_patch_created_by_whitespace_still_400(admin_api):
    res = admin_api.patch("/api/instances/", json={"created_by": " "})
    assert res.status_code == 400
    assert res.json() == {
        "created_by": [f"{LCURLY} {RCURLY} is not a valid UUID."]
    }


def test_create_workspace_logo_asset_empty_string_stores_null(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Empty Logo", "slug": "empty-logo", "logo_asset": ""},
    )
    assert res.status_code == 201
    assert res.json()["logo_asset"] is None
    with world["db"].connect() as conn:
        row = conn.execute(
            "select logo_asset_id from workspaces where slug = 'empty-logo';"
        ).fetchone()
    assert row[0] is None


def test_create_workspace_logo_asset_whitespace_still_400(admin_api):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Ws", "slug": "ws-space", "logo_asset": " "},
    )
    assert res.status_code == 400
    assert res.json() == {
        "logo_asset": [f"{LCURLY} {RCURLY} is not a valid UUID."]
    }
