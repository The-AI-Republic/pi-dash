"""Contract suite: GitLab callback uploads the provider avatar (PIDASHCONV-1323).

Django's ``complete_login_or_signup`` (``authentication/adapter/base.py:329-338``)
calls ``download_and_upload_avatar`` on signup, and ``sync_user_data``
(``base.py:256-284``) calls ``delete_old_avatar`` + ``download_and_upload_avatar``.
With working S3 that creates a ``FileAsset`` row (``entity_type=USER_AVATAR``,
``is_uploaded=True``) and sets ``users.avatar_asset_id``; sync also removes the
previous avatar asset. The Rust GitLab port downloaded the bytes and threw them
away (``let _ = download_avatar_bytes(...)``): no upload, no row, and the old
avatar FK survived a sync.

Harness choice: GitLab accepts a host override, so this module points
``GITLAB_HOST`` at a loopback stub (the PIDASHCONV-1360 seam) serving the token
+ userinfo endpoints, and drives the full initiate -> callback flow over plain
HTTP on both backends. A second stub serves the avatar image bytes (bound first
so its URL can be embedded in the userinfo payload). S3 is the contract
endpoint (LocalStack in CI): ``USE_MINIO`` unset, the test creates the contract
bucket itself and deletes it in teardown so sibling tests keep their no-bucket
upload-failure assumption. No test here contacts a real provider.

The ``GITLAB_*``/``ENABLE_GITLAB_SYNC`` ``instance_configurations`` rows are
inserted function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests run
serially; no fixture can observe the rows mid-test). Teardown lives in the
fixture (always runs, even when the callback or an early assertion fails):
leaking ``ENABLE_GITLAB_SYNC`` would silently flip sync on for every later
gitlab-callback test in the module and poison the suite.
"""

import base64
import os
import sys
import uuid
from contextlib import contextmanager
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402
from _harness import s3 as s3h  # noqa: E402

CONFIG_KEYS = (
    "GITLAB_CLIENT_ID",
    "GITLAB_CLIENT_SECRET",
    "GITLAB_HOST",
    "ENABLE_GITLAB_SYNC",
)

# 1x1 transparent PNG: both backends accept it by Content-Type alone
# (extension map lookup; the bytes are never decoded).
AVATAR_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


def _delete_config(database_url):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(CONFIG_KEYS),),
    )


def _delete_world(store, email):
    """Best-effort world teardown; file_assets first (its user FK has no
    DB-level cascade — Django cascades in Python, raw SQL does not)."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM file_assets WHERE user_id IN"
                " (SELECT id FROM users WHERE email = %s)",
                (email,),
            )
            cur.execute(
                "DELETE FROM accounts WHERE user_id IN"
                " (SELECT id FROM users WHERE email = %s)",
                (email,),
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN"
                " (SELECT id FROM users WHERE email = %s)",
                (email,),
            )
            # Django's post_save receiver auto-creates these for the
            # callback-created user (no-op on Rust, which runs no signals).
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN"
                " (SELECT id FROM users WHERE email = %s)",
                (email,),
            )
            cur.execute("DELETE FROM users WHERE email = %s", (email,))
        conn.commit()


def _delete_s3_keys(bucket, keys):
    """Best-effort per key: one slow S3 call must not skip the rest of
    teardown (nor the DB cleanup that follows it)."""
    for key in keys:
        try:
            s3h.s3_call("DELETE", f"{s3h.s3_endpoint()}/{bucket}/{key}")
        except Exception:
            pass


def _delete_bucket(bucket):
    """Best-effort bucket removal (after the keys are gone).

    Sibling callback tests assume the S3 upload fails in the contract envs
    (no bucket => NoSuchBucket => provider-URL fallback); a leaked bucket
    would flip their uploads to success and break them.
    """
    try:
        s3h.s3_call("DELETE", f"{s3h.s3_endpoint()}/{bucket}")
    except Exception:
        pass


@contextmanager
def _world(sync_enabled):
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gl1323-{tag}@example.test"
    bucket = s3h.contract_bucket()
    s3h.s3_setup_bucket(bucket)
    world = {"tag": tag, "email": email, "bucket": bucket}
    with RoutingStub({("GET", "/avatar.png"): (200, AVATAR_PNG, "image/png")}) as avatars:
        gitlab_routes = {
            ("POST", "/oauth/token"): (
                200,
                {
                    "access_token": f"stub-at-{tag}",
                    "token_type": "Bearer",
                    "expires_in": 7200,
                    "refresh_token": f"stub-rt-{tag}",
                    "created_at": 1_700_000_000,
                },
            ),
            ("GET", "/api/v4/user"): (
                200,
                {
                    "id": 424242,
                    "email": email,
                    "name": "Avatar User",
                    "username": f"gl1323-{tag}",
                    "avatar_url": f"{avatars.url}/avatar.png",
                },
            ),
        }
        with RoutingStub(gitlab_routes) as stub:
            # Reap before sow: a crashed earlier run must not leak config
            # rows into this one (or, via teardown, into sibling modules).
            _delete_config(database_url)
            try:
                store.make_config(
                    "GITLAB_CLIENT_ID", "contract-id", category="AUTHENTICATION"
                )
                store.make_config(
                    "GITLAB_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
                )
                store.make_config("GITLAB_HOST", stub.url, category="AUTHENTICATION")
                if sync_enabled:
                    store.make_config(
                        "ENABLE_GITLAB_SYNC", "1", category="AUTHENTICATION"
                    )
            except Exception:
                _delete_config(database_url)
                raise
            try:
                world["stub"] = stub
                world["avatars"] = avatars
                world["avatar_url"] = f"{avatars.url}/avatar.png"
                yield world
            finally:
                rows = db.fetchall(
                    database_url,
                    "SELECT asset FROM file_assets WHERE user_id IN"
                    " (SELECT id FROM users WHERE email = %s)",
                    (email,),
                )
                _delete_s3_keys(bucket, [r["asset"] for r in rows])
                _delete_bucket(bucket)
                _delete_config(database_url)
                _delete_world(store, email)


@pytest.fixture()
def gitlab_avatar_world(ensure_instance):
    """Fresh signup email behind a stub GitLab + avatar stub, sync off."""
    with _world(sync_enabled=False) as world:
        yield world


@pytest.fixture()
def gitlab_avatar_sync_world(ensure_instance):
    """Fresh signup email behind a stub GitLab + avatar stub, sync on."""
    with _world(sync_enabled=True) as world:
        yield world


def _drive_callback(client: httpx.Client):
    """Real initiate -> callback flow; return the callback response."""
    initiate = client.get("/auth/gitlab/")
    assert initiate.status_code == 302, (
        f"initiate: {initiate.status_code}: {initiate.text[:200]}"
    )
    state = parse_qs(urlsplit(initiate.headers["location"]).query).get("state", [None])[0]
    assert state, initiate.headers["location"]
    callback = client.get(
        "/auth/gitlab/callback/", params={"code": "contract-code", "state": state}
    )
    assert callback.status_code == 302, (
        f"callback: {callback.status_code}: {callback.text[:300]}"
    )
    location = callback.headers["location"]
    query = parse_qs(urlsplit(location).query)
    assert "error_code" not in query, location
    assert "error_message" not in query, location
    return callback


def test_gitlab_signup_uploads_avatar(client: httpx.Client, gitlab_avatar_world):
    """Signup uploads the provider avatar: one live USER_AVATAR row + FK.

    Runs against both backends. Pre-fix Rust stores only the provider URL
    (no row, FK NULL), so the row-count assertion fails; post-fix both
    backends show the row, the FK, and the S3 object.
    """
    database_url = config.database_url()
    world = gitlab_avatar_world
    email, bucket = world["email"], world["bucket"]

    _drive_callback(client)

    avatar_hits = [
        r
        for r in world["avatars"].requests
        if urlsplit(r["path"]).path == "/avatar.png"
    ]
    assert avatar_hits, (
        "backend never downloaded the avatar; upload assertions would be vacuous",
        world["avatars"].requests,
    )

    user = db.fetchone(
        database_url,
        "SELECT id, avatar, avatar_asset_id FROM users WHERE email = %s",
        (email,),
    )
    assert user is not None, "callback created the user"
    assert user["avatar"] == world["avatar_url"], user
    rows = db.fetchall(
        database_url,
        "SELECT id, asset, entity_type, is_uploaded, deleted_at, attributes, size"
        " FROM file_assets WHERE user_id = %s",
        (user["id"],),
    )
    assert len(rows) == 1, ("expected the signup avatar asset", rows)
    (row,) = rows
    assert row["entity_type"] == "USER_AVATAR", row
    assert row["is_uploaded"] is True, row
    assert row["deleted_at"] is None, row
    assert row["attributes"]["name"] == "gitlab-avatar.png", row
    assert row["attributes"]["type"] == "image/png", row
    assert row["attributes"]["size"] == len(AVATAR_PNG), row
    assert row["size"] == float(len(AVATAR_PNG)), row
    assert user["avatar_asset_id"] == row["id"], (user, rows)
    assert row["asset"].endswith("-user-avatar.png"), row
    assert s3h.s3_head_key(bucket, row["asset"]) == 200, (
        "avatar S3 object present",
        row["asset"],
    )


def test_gitlab_signup_sync_replaces_avatar(client: httpx.Client, gitlab_avatar_sync_world):
    """Signup with sync on: the signup asset is replaced, old one removed.

    Runs against both backends. The signup branch uploads asset #1, sync's
    ``delete_old_avatar`` removes its S3 object, soft-deletes the row and
    clears the FK, and the re-upload lands asset #2. Pre-fix Rust never
    uploads (no rows at all) and keeps any old FK, so the row-count
    assertion fails; post-fix both backends show one live + one
    soft-deleted row, the user pointing at the live one with a blank
    avatar string, the old S3 object gone.
    """
    database_url = config.database_url()
    world = gitlab_avatar_sync_world
    email, bucket = world["email"], world["bucket"]

    _drive_callback(client)

    avatar_hits = [
        r
        for r in world["avatars"].requests
        if urlsplit(r["path"]).path == "/avatar.png"
    ]
    assert len(avatar_hits) == 2, (
        "signup + sync each download the avatar",
        world["avatars"].requests,
    )

    user = db.fetchone(
        database_url,
        "SELECT id, avatar, avatar_asset_id FROM users WHERE email = %s",
        (email,),
    )
    assert user is not None, "callback created the user"
    rows = db.fetchall(
        database_url,
        "SELECT id, asset, deleted_at FROM file_assets"
        " WHERE user_id = %s ORDER BY created_at",
        (user["id"],),
    )
    assert len(rows) == 2, ("expected the signup asset + its sync replacement", rows)
    old, new = rows
    assert old["deleted_at"] is not None, (
        "replaced avatar asset soft-deletes (deleted_at set)",
        rows,
    )
    assert new["deleted_at"] is None, "replacement asset stays live"
    assert user["avatar_asset_id"] == new["id"], (
        "user points at the replacement asset",
        user,
        rows,
    )
    assert user["avatar"] == "", user
    assert s3h.s3_head_key(bucket, old["asset"]) == 404, (
        "old S3 object deleted",
        old["asset"],
    )
    assert s3h.s3_head_key(bucket, new["asset"]) == 200, (
        "new S3 object present",
        new["asset"],
    )
