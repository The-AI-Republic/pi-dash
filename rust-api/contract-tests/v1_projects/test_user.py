"""Contract tests: api-v1 current-user endpoint (urls/user.py, 1 URL entry)."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "user"


def test_me_shape(seed):
    body = http.get(seed["keys"][KEY], "/api/v1/users/me/").json()
    assert set(body.keys()) == {
        "avatar", "avatar_url", "display_name", "email", "first_name", "id", "last_name",
    }
    assert body["id"] == seed["owner"]["id"]
    assert body["email"] == seed["owner"]["email"]


def test_me_unauthenticated_401(seed):
    http.get(None, "/api/v1/users/me/", expect=401)


def test_me_bad_token_403(seed):
    # Quirk pinned: an *invalid* token answers 403 (not 401) carrying the
    # AuthenticationFailed message; only *missing* credentials answer 401.
    r = http.get("ct-bogus-token", "/api/v1/users/me/", expect=403)
    assert r.json() == {"detail": "Given API token is not valid"}


def test_me_null_email(seed, conn):
    # users.email is nullable (db/models/user.py:61); the serializer renders
    # null — it must not 500 (PIDASHCONV-513).
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtNull", email=None)
    assert user["email"] is None
    key = db.create_api_token(conn, user["id"], tag + "n")["token"]
    body = http.get(key, "/api/v1/users/me/").json()
    assert body["id"] == user["id"]
    assert body["email"] is None


def test_me_avatar_asset_unresolvable_renders_null(seed, conn):
    # avatar_url returns the asset URL as-is when an asset is set
    # (db/models/user.py:142-151): an unmapped entity type renders null even
    # with non-empty avatar text — no fall-through (PIDASHCONV-513).
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtAv")
    aid = db.create_file_asset(conn, "DRAFT_ISSUE_ATTACHMENT")
    db.set_user_avatar(conn, user["id"], asset_id=aid, avatar="https://cdn.example/legacy.png")
    key = db.create_api_token(conn, user["id"], tag + "v")["token"]
    body = http.get(key, "/api/v1/users/me/").json()
    assert body["avatar"] == "https://cdn.example/legacy.png"
    assert body["avatar_url"] is None
    # Sanity: a mapped type on the same row resolves, proving the seed shape.
    aid2 = db.create_file_asset(conn, "USER_AVATAR")
    db.set_user_avatar(conn, user["id"], asset_id=aid2, avatar="https://cdn.example/legacy.png")
    body = http.get(key, "/api/v1/users/me/").json()
    assert body["avatar_url"] == f"/api/assets/v2/static/{aid2}/"
