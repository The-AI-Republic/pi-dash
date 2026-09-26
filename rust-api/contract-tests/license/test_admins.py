# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance admins: list/create/delete, me, session, signup-screen-visited."""

ME_KEYS = {
    "id",
    "avatar",
    "avatar_url",
    "cover_image",
    "date_joined",
    "display_name",
    "email",
    "first_name",
    "last_name",
    "is_active",
    "is_bot",
    "is_email_verified",
    "user_timezone",
    "username",
    "is_password_autoset",
}


def test_list_admins_shape(admin_api, world):
    res = admin_api.get("/api/instances/admins/")
    assert res.status_code == 200
    (entry,) = res.json()
    assert entry["user_detail"]["email"] == "admin@example.com"
    assert entry["role"] == 20
    assert entry["instance"] == world["instance"]["id"]


def test_create_admin(admin_api, world):
    newcomer = world["db"].make_user("newcomer@example.com")
    res = admin_api.post("/api/instances/admins/", json={"email": "newcomer@example.com", "role": 20})
    assert res.status_code == 201
    assert res.json()["user_detail"]["email"] == "newcomer@example.com"
    with world["db"].connect() as conn:
        count = conn.execute(
            "select count(*) from instance_admins where user_id = %s;", (newcomer["id"],)
        ).fetchone()[0]
    assert count == 1


def test_create_admin_unknown_user(admin_api):
    res = admin_api.post("/api/instances/admins/", json={"email": "ghost@example.com"})
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_create_admin_missing_email(admin_api):
    res = admin_api.post("/api/instances/admins/", json={})
    assert res.status_code == 400
    assert res.json() == {"error": "Email is required"}


def test_delete_admin_soft_deletes(admin_api, world):
    # Deletes here are soft: the row stays with deleted_at set and drops
    # out of the default manager (SoftDeletionQuerySet).
    doomed = world["db"].make_user("doomed@example.com")
    row = world["db"].make_admin(world["instance"]["id"], doomed["id"])
    res = admin_api.delete(f"/api/instances/admins/{row['id']}/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        deleted_at = conn.execute(
            "select deleted_at from instance_admins where id = %s;", (row["id"],)
        ).fetchone()[0]
        live = conn.execute(
            "select count(*) from instance_admins where id = %s and deleted_at is null;",
            (row["id"],),
        ).fetchone()[0]
    assert deleted_at is not None
    assert live == 0
    emails = [entry["user_detail"]["email"] for entry in admin_api.get("/api/instances/admins/").json()]
    assert "doomed@example.com" not in emails


def test_delete_unknown_admin_is_idempotent(admin_api):
    res = admin_api.delete("/api/instances/admins/00000000-0000-0000-0000-000000000000/")
    assert res.status_code == 204


def test_me_shape(admin_api):
    res = admin_api.get("/api/instances/admins/me/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == ME_KEYS
    assert body["email"] == "admin@example.com"


def test_session_reports_user(admin_api):
    body = admin_api.get("/api/instances/admins/session/").json()
    assert body["is_authenticated"] is True
    assert body["user"]["email"] == "admin@example.com"


def test_session_anonymous(anon_api):
    res = anon_api.get("/api/instances/admins/session/")
    assert res.status_code == 200
    assert res.json() == {"is_authenticated": False}


def test_signup_screen_visited_marks_instance(admin_api, world):
    res = admin_api.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        visited = conn.execute("select is_signup_screen_visited from instances;").fetchone()[0]
    assert visited is True


def test_signup_screen_visited_without_instance(anon_api):
    res = anon_api.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 400
    assert res.json() == {"error": "Instance is not configured"}
