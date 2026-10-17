# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Exotic JSON values render exactly like Python str()/repr() (PIDASHCONV-1116).

Float tokens, huge integers, backslashes/quotes/control chars, bools and
composites cross JSON→Python-string bridges on instance PATCH, config PATCH
and workspace POST. Every test asserts the stored row plus the response body.
"""

HUGE = 2**100
HUGE_DECIMAL = str(HUGE)
NEG_HUGE_DECIMAL = str(-HUGE)

JSON_HEADERS = {"Content-Type": "application/json"}


def _instance_name_row(db):
    with db.connect() as conn:
        return conn.execute("select instance_name from instances;").fetchone()[0]


def _config_row(db, key):
    with db.connect() as conn:
        return conn.execute(
            "select value from instance_configurations where key = %s;", (key,)
        ).fetchone()[0]


def _member_role_row(db, slug):
    with db.connect() as conn:
        return conn.execute(
            "select company_role from workspace_members wm join workspaces w"
            " on w.id = wm.workspace_id where w.slug = %s;",
            (slug,),
        ).fetchone()[0]


def test_instance_name_float_exponent_spelling(admin_api, world):
    res = admin_api.patch(
        "/api/instances/", content=b'{"instance_name": 1e3}', headers=JSON_HEADERS
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "1000.0"
    assert _instance_name_row(world["db"]) == "1000.0"


def test_instance_name_float_trailing_zero(admin_api, world):
    res = admin_api.patch(
        "/api/instances/", content=b'{"instance_name": 1.50}', headers=JSON_HEADERS
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "1.5"
    assert _instance_name_row(world["db"]) == "1.5"


def test_config_huge_int_exact_decimal(admin_api, world):
    world["db"].make_config("EXOTIC_HUGE", "old", category="general")
    res = admin_api.patch("/api/instances/configurations/", json={"EXOTIC_HUGE": HUGE})
    assert res.status_code == 200
    assert res.json()[0]["value"] == HUGE_DECIMAL
    assert _config_row(world["db"], "EXOTIC_HUGE") == HUGE_DECIMAL


def test_config_float_exponent_spelling(admin_api, world):
    world["db"].make_config("EXOTIC_FLOAT", "old", category="general")
    res = admin_api.patch(
        "/api/instances/configurations/",
        content=b'{"EXOTIC_FLOAT": 1e3}',
        headers=JSON_HEADERS,
    )
    assert res.status_code == 200
    assert res.json()[0]["value"] == "1000.0"
    assert _config_row(world["db"], "EXOTIC_FLOAT") == "1000.0"


def test_config_nested_backslash_quote_repr(admin_api, world):
    world["db"].make_config("EXOTIC_REPR", "old", category="general")
    res = admin_api.patch(
        "/api/instances/configurations/",
        content=b'{"EXOTIC_REPR": ["a\\\\b\'c"]}',
        headers=JSON_HEADERS,
    )
    expected = repr(["a\\b'c"])
    assert res.status_code == 200
    assert res.json()[0]["value"] == expected
    assert _config_row(world["db"], "EXOTIC_REPR") == expected


def test_config_nested_control_chars_repr(admin_api, world):
    world["db"].make_config("EXOTIC_CTRL", "old", category="general")
    res = admin_api.patch(
        "/api/instances/configurations/",
        content=b'{"EXOTIC_CTRL": ["a\\u0001b\\u007f"]}',
        headers=JSON_HEADERS,
    )
    expected = repr(["a\x01b\x7f"])
    assert res.status_code == 200
    assert res.json()[0]["value"] == expected
    assert _config_row(world["db"], "EXOTIC_CTRL") == expected


def test_workspace_company_role_true(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Bool", "slug": "ws-bool", "company_role": True},
    )
    assert res.status_code == 201
    assert _member_role_row(world["db"], "ws-bool") == "True"


def test_workspace_company_role_false(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Bool", "slug": "ws-bool-f", "company_role": False},
    )
    assert res.status_code == 201
    assert _member_role_row(world["db"], "ws-bool-f") == "False"


def test_workspace_company_role_list(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "List", "slug": "ws-list", "company_role": [1]},
    )
    assert res.status_code == 201
    assert _member_role_row(world["db"], "ws-list") == "[1]"


def test_workspace_company_role_dict(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Dict", "slug": "ws-dict", "company_role": {"a": 1}},
    )
    assert res.status_code == 201
    assert _member_role_row(world["db"], "ws-dict") == "{'a': 1}"


def test_workspace_company_role_huge_int(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Huge", "slug": "ws-huge", "company_role": HUGE},
    )
    assert res.status_code == 201
    assert _member_role_row(world["db"], "ws-huge") == HUGE_DECIMAL


def test_workspace_logo_asset_huge_int(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Asset", "slug": "ws-asset", "logo_asset": HUGE},
    )
    assert res.status_code == 400
    assert res.json() == {
        "logo_asset": [f'Invalid pk "{HUGE_DECIMAL}" - object does not exist.']
    }


def test_workspace_logo_asset_negative_huge_int(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Asset", "slug": "ws-asset-neg", "logo_asset": -HUGE},
    )
    assert res.status_code == 400
    assert res.json() == {
        "logo_asset": [f"“{NEG_HUGE_DECIMAL}” is not a valid UUID."]
    }
