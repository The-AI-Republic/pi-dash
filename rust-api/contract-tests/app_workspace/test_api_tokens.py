# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""API tokens: create / list / detail / patch / delete over the app session.

Covers the denied-permission floor (anonymous -> 401 on every method) and
the tenant-isolation floor (another user's token reads as 404).
"""

import uuid

CREATE_KEYS = {
    "allowed_rate_limit",
    "created_at",
    "created_by",
    "deleted_at",
    "description",
    "expired_at",
    "id",
    "is_active",
    "is_service",
    "label",
    "last_used",
    "token",
    "updated_at",
    "updated_by",
    "user",
    "user_type",
    "workspace",
}

READ_KEYS = CREATE_KEYS - {"token"}

DENIED = {"detail": "Authentication credentials were not provided."}


def test_create_token_shape(user_api, world):
    res = user_api.post(
        "/api/users/api-tokens/", json={"label": "Deploy", "description": "CI deploys"}
    )
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == CREATE_KEYS
    assert body["label"] == "Deploy"
    assert body["description"] == "CI deploys"
    assert body["user_type"] == 0
    assert body["user"] == world["user"]["id"]
    assert body["token"].startswith("pi_dash_api_")
    assert body["is_service"] is False
    assert body["workspace"] is None
    with world["db"].connect() as conn:
        row = conn.execute(
            "select user_id, label from api_tokens where id = %s;", (body["id"],)
        ).fetchone()
    assert str(row[0]) == world["user"]["id"]
    assert row[1] == "Deploy"


def test_create_token_minimal_data(user_api):
    res = user_api.post("/api/users/api-tokens/", json={})
    assert res.status_code == 201
    body = res.json()
    assert "token" in body
    assert len(body["label"]) == 32  # uuid4().hex default
    assert body["description"] == ""


def test_create_token_with_expiry(user_api, world):
    res = user_api.post(
        "/api/users/api-tokens/",
        json={"label": "Expiring", "expired_at": "2030-01-01T00:00:00Z"},
    )
    assert res.status_code == 201
    with world["db"].connect() as conn:
        row = conn.execute(
            "select expired_at from api_tokens where id = %s;", (res.json()["id"],)
        ).fetchone()
    assert row[0] is not None


def test_create_token_bot_user_marks_user_type(bot_api):
    res = bot_api.post("/api/users/api-tokens/", json={"label": "Bot token"})
    assert res.status_code == 201
    assert res.json()["user_type"] == 1


def test_list_tokens_shape_and_excludes_service(user_api, world):
    world["db"].make_api_token(world["user"]["id"], label="Token 1")
    world["db"].make_api_token(world["user"]["id"], label="Token 2")
    world["db"].make_api_token(world["user"]["id"], label="Service", is_service=True)
    res = user_api.get("/api/users/api-tokens/")
    assert res.status_code == 200
    body = res.json()
    assert isinstance(body, list)
    assert {t["label"] for t in body} == {"Token 1", "Token 2"}
    assert all(set(t.keys()) == READ_KEYS for t in body)
    assert all(t["is_service"] is False for t in body)
    assert all("token" not in t for t in body)


def test_list_tokens_empty(user_api):
    assert user_api.get("/api/users/api-tokens/").json() == []


def test_get_token_detail_shape(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"], label="Detail")
    res = user_api.get(f"/api/users/api-tokens/{seed['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == READ_KEYS
    assert body["id"] == seed["id"]
    assert body["label"] == "Detail"
    assert "token" not in body


def test_get_nonexistent_token_is_404(user_api):
    res = user_api.get(f"/api/users/api-tokens/{uuid.uuid4()}/")
    assert res.status_code == 404


def test_get_other_users_token_is_404(user_api, world):
    # Tenant isolation: another user's token reads as missing, not forbidden.
    seed = world["db"].make_api_token(world["other"]["id"], label="Other")
    res = user_api.get(f"/api/users/api-tokens/{seed['id']}/")
    assert res.status_code == 404


def test_delete_token_soft_deletes(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"])
    res = user_api.delete(f"/api/users/api-tokens/{seed['id']}/")
    assert res.status_code == 204
    # Soft delete: the row is kept with deleted_at set, and drops from reads.
    with world["db"].connect() as conn:
        deleted_at = conn.execute(
            "select deleted_at from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert deleted_at is not None
    assert user_api.get(f"/api/users/api-tokens/{seed['id']}/").status_code == 404
    assert user_api.get("/api/users/api-tokens/").json() == []


def test_delete_nonexistent_token_is_404(user_api):
    assert user_api.delete(f"/api/users/api-tokens/{uuid.uuid4()}/").status_code == 404


def test_delete_other_users_token_is_404_and_kept(user_api, world):
    seed = world["db"].make_api_token(world["other"]["id"])
    assert user_api.delete(f"/api/users/api-tokens/{seed['id']}/").status_code == 404
    with world["db"].connect() as conn:
        count = conn.execute(
            "select count(*) from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert count == 1


def test_delete_service_token_is_404_and_kept(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"], is_service=True)
    assert user_api.delete(f"/api/users/api-tokens/{seed['id']}/").status_code == 404
    with world["db"].connect() as conn:
        count = conn.execute(
            "select count(*) from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert count == 1


def test_patch_token(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"], label="Old")
    res = user_api.patch(
        f"/api/users/api-tokens/{seed['id']}/",
        json={"label": "New", "description": "Updated"},
    )
    assert res.status_code == 200
    assert res.json()["label"] == "New"
    assert res.json()["description"] == "Updated"
    with world["db"].connect() as conn:
        row = conn.execute(
            "select label, description from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()
    assert row == ("New", "Updated")


def test_patch_token_partial_update(user_api, world):
    world["db"].make_api_token(world["user"]["id"], label="Old", description="Keep")
    seed_id = None
    with world["db"].connect() as conn:
        seed_id = conn.execute("select id from api_tokens;").fetchone()[0]
    res = user_api.patch(f"/api/users/api-tokens/{seed_id}/", json={"label": "Only"})
    assert res.status_code == 200
    assert res.json()["label"] == "Only"
    assert res.json()["description"] == "Keep"


def test_patch_nonexistent_token_is_404(user_api):
    res = user_api.patch(
        f"/api/users/api-tokens/{uuid.uuid4()}/", json={"label": "New"}
    )
    assert res.status_code == 404


def test_patch_other_users_token_is_404_and_unchanged(user_api, world):
    seed = world["db"].make_api_token(world["other"]["id"], label="Other")
    res = user_api.patch(
        f"/api/users/api-tokens/{seed['id']}/", json={"label": "Hacked"}
    )
    assert res.status_code == 404
    with world["db"].connect() as conn:
        label = conn.execute(
            "select label from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert label == "Other"


def test_patch_cannot_modify_token_value(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"], token="pi_dash_api_original")
    res = user_api.patch(
        f"/api/users/api-tokens/{seed['id']}/", json={"token": "pi_dash_api_malicious"}
    )
    assert res.status_code == 200
    with world["db"].connect() as conn:
        token = conn.execute(
            "select token from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert token == "pi_dash_api_original"


def test_patch_cannot_modify_user_type(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"])
    res = user_api.patch(f"/api/users/api-tokens/{seed['id']}/", json={"user_type": 1})
    assert res.status_code == 200
    with world["db"].connect() as conn:
        user_type = conn.execute(
            "select user_type from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert user_type == 0


def test_patch_cannot_modify_service_token(user_api, world):
    seed = world["db"].make_api_token(world["user"]["id"], label="Service", is_service=True)
    res = user_api.patch(
        f"/api/users/api-tokens/{seed['id']}/", json={"label": "Hacked"}
    )
    assert res.status_code == 404
    with world["db"].connect() as conn:
        label = conn.execute(
            "select label from api_tokens where id = %s;", (seed["id"],)
        ).fetchone()[0]
    assert label == "Service"


def test_anonymous_is_denied_on_every_method(anon_api):
    pk = uuid.uuid4()
    assert anon_api.get("/api/users/api-tokens/").status_code == 401
    assert anon_api.get("/api/users/api-tokens/").json() == DENIED
    assert anon_api.post("/api/users/api-tokens/", json={"label": "x"}).status_code == 401
    assert anon_api.get(f"/api/users/api-tokens/{pk}/").status_code == 401
    assert anon_api.patch(f"/api/users/api-tokens/{pk}/", json={"label": "x"}).status_code == 401
    assert anon_api.delete(f"/api/users/api-tokens/{pk}/").status_code == 401
