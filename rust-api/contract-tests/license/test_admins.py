# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance admins: list/create/delete, me, session, signup-screen-visited."""

import pytest

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


@pytest.mark.parametrize("role", ["abc", {}, [], None])
def test_create_admin_bad_role_unknown_user_is_404(admin_api, role):
    # The role is only touched at .create(), AFTER the user lookup, so an
    # unknown email 404s no matter how unparsable the role is (PIDASHCONV-1120a).
    res = admin_api.post("/api/instances/admins/", json={"role": role, "email": "nobody@example.com"})
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_create_admin_max_boundary_float_truncates(admin_api, world):
    # int(2147483647.5) -> 2147483647: Python truncates toward zero BEFORE
    # the range check, so the boundary float stores (PIDASHCONV-1120b).
    # (The 201 body echoes the raw 2147483647.5 input attribute on Django;
    # that response-rendering quirk is out of scope here — only the status
    # and the stored value are asserted.)
    newcomer = world["db"].make_user("floatmax@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": 2147483647.5, "email": newcomer["email"]})
    assert res.status_code == 201
    with world["db"].connect() as conn:
        stored = conn.execute(
            "select role from instance_admins where user_id = %s;", (newcomer["id"],)
        ).fetchone()[0]
    assert stored == 2147483647


def test_create_admin_min_boundary_float_rejected_by_check(admin_api, world):
    # int(-2147483648.5) -> -2147483648 fits i32, but the
    # instance_admins_role_check (role >= 0) rejects it: IntegrityError ->
    # 400 invalid-payload (PIDASHCONV-1120b). PIDASHCONV-1120 expected 201
    # assuming no CHECK; the migrated schema has the CHECK and live Django
    # 400s, so 400 is the oracle behavior on both backends.
    newcomer = world["db"].make_user("floatmin@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": -2147483648.5, "email": newcomer["email"]})
    assert res.status_code == 400
    assert res.json() == {"error": "The payload is not valid"}
    with world["db"].connect() as conn:
        count = conn.execute(
            "select count(*) from instance_admins where user_id = %s;", (newcomer["id"],)
        ).fetchone()[0]
    assert count == 0


@pytest.mark.parametrize(
    "role,expected_bytes,stored",
    [
        # The 201 body echoes the RAW request role (DRF ChoiceField over the
        # still-raw in-memory attribute), not the stored i32 (PIDASHCONV-1129).
        # Parsed checks alone are vacuous here (20.0 == 20), so every case
        # also pins the exact response bytes.
        (2147483647.5, b'"role":2147483647.5', 2147483647),
        (20.5, b'"role":20.5', 20),
        (20.0, b'"role":20.0', 20),
        (0.00001, b'"role":1e-05', 0),
        (-0.5, b'"role":-0.5', 0),
    ],
)
def test_create_admin_float_role_echoes_raw(admin_api, world, role, expected_bytes, stored):
    newcomer = world["db"].make_user("floatecho@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": role, "email": newcomer["email"]})
    assert res.status_code == 201
    assert isinstance(res.json()["role"], float)
    assert expected_bytes in res.content
    with world["db"].connect() as conn:
        assert (
            conn.execute(
                "select role from instance_admins where user_id = %s;", (newcomer["id"],)
            ).fetchone()[0]
            == stored
        )


def test_create_admin_float_token_normalizes_to_python_repr(admin_api, world):
    # The echo re-renders the parsed float (Python repr), it does not repeat
    # the request token: 1E-5 arrives spelled with a capital E and echoes as
    # 1e-05 on both backends (PIDASHCONV-1129).
    newcomer = world["db"].make_user("floattoken@example.com")
    raw = '{"email": "%s", "role": 1E-5}' % newcomer["email"]
    res = admin_api.post(
        "/api/instances/admins/",
        content=raw.encode(),
        headers={"Content-Type": "application/json"},
    )
    assert res.status_code == 201
    assert res.json()["role"] == 1e-05
    assert b'"role":1e-05' in res.content
    with world["db"].connect() as conn:
        assert (
            conn.execute(
                "select role from instance_admins where user_id = %s;", (newcomer["id"],)
            ).fetchone()[0]
            == 0
        )


@pytest.mark.parametrize(
    "role,expected_bytes,stored",
    [
        (True, b'"role":true', 1),
        (False, b'"role":false', 0),
    ],
)
def test_create_admin_bool_role_echoes_raw(admin_api, world, role, expected_bytes, stored):
    # ChoiceField str(True) is "True", not a choice, so the bool echoes as a
    # JSON bool while the column stores 1/0 (PIDASHCONV-1129). `is` (not
    # `==`) because 1 == True.
    newcomer = world["db"].make_user("boolecho@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": role, "email": newcomer["email"]})
    assert res.status_code == 201
    assert res.json()["role"] is role
    assert expected_bytes in res.content
    with world["db"].connect() as conn:
        assert (
            conn.execute(
                "select role from instance_admins where user_id = %s;", (newcomer["id"],)
            ).fetchone()[0]
            == stored
        )


@pytest.mark.parametrize("role", [" 20 ", "+20", "020", "\u00a020"])
def test_create_admin_padded_string_role_echoes_raw(admin_api, world, role):
    # Only the exact string "20" maps to the 20 number; every other
    # int-parseable spelling echoes as a JSON string (PIDASHCONV-1129).
    newcomer = world["db"].make_user("stringecho@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": role, "email": newcomer["email"]})
    assert res.status_code == 201
    assert res.json()["role"] == role
    assert ('"role":"%s"' % role).encode() in res.content
    with world["db"].connect() as conn:
        assert (
            conn.execute(
                "select role from instance_admins where user_id = %s;", (newcomer["id"],)
            ).fetchone()[0]
            == 20
        )


@pytest.mark.parametrize(
    "role,expected,expected_bytes",
    [
        (20, 20, b'"role":20'),
        ("20", 20, b'"role":20'),
        (1, 1, b'"role":1'),
    ],
)
def test_create_admin_canonical_role_renders_int(admin_api, world, role, expected, expected_bytes):
    # Controls: canonical inputs render as JSON ints on both backends — the
    # ChoiceField map (str "20" -> 20) and non-choice ints echo unchanged
    # (PIDASHCONV-1129 guards against over-echoing).
    newcomer = world["db"].make_user("canonecho@example.com")
    res = admin_api.post("/api/instances/admins/", json={"role": role, "email": newcomer["email"]})
    assert res.status_code == 201
    assert res.json()["role"] == expected
    assert isinstance(res.json()["role"], int)
    assert expected_bytes in res.content


def test_create_admin_missing_role_defaults_and_renders_20(admin_api, world):
    newcomer = world["db"].make_user("defaultrole@example.com")
    res = admin_api.post("/api/instances/admins/", json={"email": newcomer["email"]})
    assert res.status_code == 201
    assert res.json()["role"] == 20
    assert b'"role":20' in res.content


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


def _non_canonical_spellings(row_id):
    # Every spelling `uuid.UUID()` (and Rust's `Uuid::parse_str`) accepts
    # but Django's `<uuid:pk>` converter (`[0-9a-f]{8}-…`, matched
    # case-sensitively) does not route.
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
        "braced": "{" + row_id + "}",
        "urn": "urn:uuid:" + row_id,
    }


def _assert_resolver_404(res):
    # Resolving the URL precedes auth and the view entirely: the answer is
    # the resolver's HTML 404, never DRF's JSON body or a 401/403/204.
    assert res.status_code == 404, res.text
    assert res.headers["content-type"].startswith("text/html"), res.headers.get(
        "content-type"
    )


def _admin_row_live(db, row_id):
    with db.connect() as conn:
        return conn.execute(
            "select count(*) from instance_admins where id = %s and deleted_at is null;",
            (row_id,),
        ).fetchone()[0]


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_delete_admin_non_canonical_spelling_404s_without_soft_delete(
    admin_api, world, spelling
):
    # PIDASHCONV-1132: an admin DELETE of a non-canonical spelling must
    # not reach the view. Rust parses the segment with Uuid::parse_str,
    # which accepts all four spellings, so without the canonical gate it
    # answers 204 and soft-deletes the row Django refuses to route.
    db = world["db"]
    doomed = db.make_user("doomed@example.com")
    row = db.make_admin(world["instance"]["id"], doomed["id"])
    segment = _non_canonical_spellings(row["id"])[spelling]
    res = admin_api.delete(f"/api/instances/admins/{segment}/")
    _assert_resolver_404(res)
    assert _admin_row_live(db, row["id"]) == 1, spelling


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_delete_admin_non_canonical_spelling_anonymous_404s(anon_api, world, spelling):
    # Resolver 404 precedes auth: an anonymous caller gets 404, not 401.
    db = world["db"]
    doomed = db.make_user("doomed@example.com")
    row = db.make_admin(world["instance"]["id"], doomed["id"])
    segment = _non_canonical_spellings(row["id"])[spelling]
    res = anon_api.delete(f"/api/instances/admins/{segment}/")
    _assert_resolver_404(res)
    assert _admin_row_live(db, row["id"]) == 1, spelling
