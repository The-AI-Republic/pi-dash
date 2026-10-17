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


def test_signup_screen_visited_anonymous_nulls_both_audit_columns(anon_api, world):
    # Django BaseModel.save NULLs BOTH created_by and updated_by on
    # anonymous saves, even on updates (db/models/base.py:31-33) — the
    # pre-existing creator is wiped, not preserved.
    db = world["db"]
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "update instances set created_by_id = %s, updated_by_id = %s;",
                (world["admin"]["id"], world["admin"]["id"]),
            )
        conn.commit()
    res = anon_api.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 204
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("select created_by_id, updated_by_id from instances;")
            assert cur.fetchone() == (None, None)


def test_signup_screen_visited_without_instance(anon_api):
    res = anon_api.post("/api/instances/admins/sign-up-screen-visited/")
    assert res.status_code == 400
    assert res.json() == {"error": "Instance is not configured"}


def test_collection_put_is_django_405(admin_api):
    # Unowned methods proxy, so Django's own DRF 405 body is preserved.
    res = admin_api.put("/api/instances/admins/", json={})
    assert res.status_code == 405
    assert res.json() == {"detail": 'Method "PUT" not allowed.'}


def test_member_detail_post_is_django_500(admin_api, world):
    # The detail path mounts the same view class, whose post() takes no
    # pk: the kwarg raises TypeError (500), before the body is ever read.
    res = admin_api.post(f"/api/instances/admins/{world['admin']['id']}/", json={})
    assert res.status_code == 500


def test_member_detail_get_is_django_500(admin_api, world):
    # Same TypeError as POST: get() takes no pk either.
    res = admin_api.get(f"/api/instances/admins/{world['admin']['id']}/")
    assert res.status_code == 500


def test_collection_delete_is_django_500(admin_api):
    # Mirror image: delete() requires pk, so the collection path 500s.
    res = admin_api.delete("/api/instances/admins/")
    assert res.status_code == 500


def test_head_on_list_matches_django(admin_api):
    # Django's View.setup() aliases head=get, so HEAD runs the GET
    # handler and answers 200 (empty body); the explicit HEAD proxy arm
    # preserves those exact bytes instead of axum's auto-HEAD.
    res = admin_api.head("/api/instances/admins/")
    assert res.status_code == 200
    assert res.content == b""


def test_options_proxies_drf_metadata(admin_api):
    res = admin_api.options("/api/instances/admins/")
    assert res.status_code == 200
    assert res.json()["name"] == "Instance Admin Endpoint"


def test_delete_non_uuid_pk_is_django_404(admin_api):
    # The <uuid:pk> converter rejects non-UUIDs before the view; Rust
    # falls through to the proxy for Django's own 404.
    res = admin_api.delete("/api/instances/admins/not-a-uuid/")
    assert res.status_code == 404
