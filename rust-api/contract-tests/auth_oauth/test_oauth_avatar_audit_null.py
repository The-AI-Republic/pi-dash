"""Contract suite: OAuth signup avatar audit columns are NULL (PIDASHCONV-1730).

Django's ``download_and_upload_avatar``
(``authentication/adapter/base.py:202-211``) passes ``created_by=user`` to
``FileAsset.objects.create``, but ``BaseModel.save``
(``db/models/base.py:21-42``) overwrites it: the callback runs before
``login()``, so the crum current user is anonymous and ``save`` stamps
``created_by = None; updated_by = None``. The Rust port
(``crates/api/src/auth_oauth/mod.rs``, ``oauth_google.rs``,
``oauth_gitea.rs``) wrote ``created_by_id = <new user>`` instead, and
contradicted its own ``soft_delete_avatar_asset`` which already nulls both
columns "with the anonymous callback user".

Harness choice: same as ``test_gitlab_avatar_upload.py`` — GitLab accepts a
host override, so this module points ``GITLAB_HOST`` at a loopback stub
serving token + userinfo, drives the full initiate -> callback flow over
plain HTTP on both backends, and asserts on the DB row directly. S3 is the
contract endpoint (local MinIO here, LocalStack in CI); the test creates the
contract bucket itself and deletes it in teardown. No test here contacts a
real provider.

The ``GITLAB_*`` ``instance_configurations`` rows are inserted
function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module.
"""

import base64
import os
import sys
import uuid
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


@pytest.fixture()
def gitlab_avatar_audit_world(ensure_instance):
    """Fresh signup email behind a stub GitLab + avatar stub, sync off."""
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gl1730-{tag}@example.test"
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
                    "name": "Audit User",
                    "username": f"gl1730-{tag}",
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
            except Exception:
                _delete_config(database_url)
                raise
            try:
                world["stub"] = stub
                world["avatars"] = avatars
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


def test_gitlab_signup_avatar_audit_columns_null(
    client: httpx.Client, gitlab_avatar_audit_world
):
    """Signup avatar row: created_by_id and updated_by_id are NULL.

    Runs against both backends. Pre-fix Rust stores ``created_by_id`` =
    the new user, so the NULL assertion fails; Django's anonymous
    ``BaseModel.save`` writes NULL, so it passes.
    """
    database_url = config.database_url()
    world = gitlab_avatar_audit_world
    email = world["email"]

    _drive_callback(client)

    avatar_hits = [
        r
        for r in world["avatars"].requests
        if urlsplit(r["path"]).path == "/avatar.png"
    ]
    assert avatar_hits, (
        "backend never downloaded the avatar; audit assertions would be vacuous",
        world["avatars"].requests,
    )

    user = db.fetchone(
        database_url,
        "SELECT id FROM users WHERE email = %s",
        (email,),
    )
    assert user is not None, "callback created the user"
    rows = db.fetchall(
        database_url,
        "SELECT id, entity_type, created_by_id, updated_by_id"
        " FROM file_assets WHERE user_id = %s",
        (user["id"],),
    )
    assert len(rows) == 1, ("expected the signup avatar asset", rows)
    (row,) = rows
    assert row["entity_type"] == "USER_AVATAR", row
    assert row["created_by_id"] is None, (
        "created_by_id is NULL (anonymous callback save), not the new user",
        row,
        str(user["id"]),
    )
    assert row["updated_by_id"] is None, (
        "updated_by_id is NULL",
        row,
    )
