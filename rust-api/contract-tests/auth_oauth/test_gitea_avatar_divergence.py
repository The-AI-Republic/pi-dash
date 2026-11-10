"""Contract suite: Gitea avatar helpers match Django (PIDASHCONV-1729).

Django's ``Adapter.download_and_upload_avatar`` / ``delete_old_avatar``
(``authentication/adapter/base.py:139-254``) store the bare
``{uuid4hex}-user-avatar.{ext}`` key, reject ``Content-Type`` values with
parameters by exact match, and soft-delete on any answered S3 DELETE.
Gitea kept private copies that diverged (prefixed ``user-{uuid}-`` key,
``;``-split Content-Type, 2xx/404 delete gate). The fix makes Gitea call
the shared ``auth_oauth/mod.rs`` helpers with provider ``"gitea"`` like
GitHub/GitLab.

Harness: Gitea accepts a host override, so this module points
``GITEA_HOST`` at a loopback stub serving token + userinfo and drives
the full app-gitea signup callback over plain HTTP on both backends
(the PIDASHCONV-1089 pattern). A second stub serves the avatar bytes.
S3 is the contract endpoint (LocalStack in CI, MinIO locally):
``USE_MINIO`` unset, ``AWS_S3_ENDPOINT_URL`` set; the test creates the
contract bucket itself and deletes it in teardown.

The ``GITEA_*`` / ``ENABLE_GITEA_SYNC`` rows are function-scoped and
deleted in teardown. No test contacts a real provider.
"""

import base64
import os
import re
import sys
import uuid
from contextlib import contextmanager
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402
from _harness import s3 as s3h  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
RUST_AUTH = REPO_ROOT / "rust-api" / "crates" / "api" / "src" / "auth_oauth"
GITEA_RS = RUST_AUTH / "oauth_gitea.rs"

CONFIG_KEYS = (
    "GITEA_CLIENT_ID",
    "GITEA_CLIENT_SECRET",
    "GITEA_HOST",
    "ENABLE_GITEA_SYNC",
)

AVATAR_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)

BARE_KEY = re.compile(r"^[0-9a-f]{32}-user-avatar\.[a-z0-9]+$")


def test_gitea_delegates_to_shared_avatar_helpers():
    """Gitea's avatar work goes through the shared helpers (static pin).

    Fails before the fix (private ``download_and_upload_avatar``,
    prefixed key and ``0x2xx/404`` gate in ``oauth_gitea.rs``); passes
    after. Runs on every suite invocation (same source on both backends).
    """
    src = GITEA_RS.read_text()
    assert "super::download_and_upload_avatar(" in src, (
        "gitea must call the shared download helper"
    )
    assert '"gitea"' in src, "gitea must pass provider \"gitea\""
    assert "super::delete_old_avatar(" in src, (
        "gitea must call the shared delete helper"
    )
    assert ".split(';')" not in src, (
        "gitea must not split Content-Type on ';' (exact match like Django)"
    )
    assert 'format!("user-{}-{filename}"' not in src, (
        "gitea must not prefix the S3 key with user-{uuid}-"
    )
    assert "response.status().is_success() || response.status() == 404" not in src, (
        "gitea must not gate the soft-delete on 2xx/404"
    )
    assert "pub async fn download_and_upload_avatar" not in src, (
        "gitea's private download copy must be gone"
    )
    assert "pub async fn delete_old_avatar" not in src, (
        "gitea's private delete copy must be gone"
    )
    assert "pub async fn insert_file_asset" not in src, (
        "gitea's private insert copy must be gone"
    )


def _delete_config(database_url):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(CONFIG_KEYS),),
    )


def _delete_world(store, email):
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
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN"
                " (SELECT id FROM users WHERE email = %s)",
                (email,),
            )
            cur.execute("DELETE FROM users WHERE email = %s", (email,))
        conn.commit()


def _delete_s3_keys(bucket, keys):
    for key in keys:
        try:
            s3h.s3_call("DELETE", f"{s3h.s3_endpoint()}/{bucket}/{key}")
        except Exception:
            pass


def _delete_bucket(bucket):
    try:
        s3h.s3_call("DELETE", f"{s3h.s3_endpoint()}/{bucket}")
    except Exception:
        pass


@contextmanager
def _world(sync_enabled, content_type="image/png"):
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gt1729-{tag}@example.test"
    bucket = s3h.contract_bucket()
    s3h.s3_setup_bucket(bucket)
    world = {"tag": tag, "email": email, "bucket": bucket}
    with RoutingStub(
        {("GET", "/avatar.png"): (200, AVATAR_PNG, content_type)}
    ) as avatars:
        gitea_routes = {
            ("POST", "/login/oauth/access_token"): (
                200,
                {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
            ),
            ("GET", "/api/v1/user"): (
                200,
                {
                    "id": 424242,
                    "email": email,
                    "login": f"gt1729-{tag}",
                    "full_name": "Avatar User",
                    "avatar_url": f"{avatars.url}/avatar.png",
                },
            ),
            ("GET", "/api/v1/user/emails"): (
                200,
                [{"email": email, "primary": True, "verified": True}],
            ),
        }
        with RoutingStub(gitea_routes) as stub:
            _delete_config(database_url)
            try:
                store.make_config(
                    "GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION"
                )
                store.make_config(
                    "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
                )
                store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
                if sync_enabled:
                    store.make_config(
                        "ENABLE_GITEA_SYNC", "1", category="AUTHENTICATION"
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


def _drive_callback(client: httpx.Client):
    initiate = client.get("/auth/gitea/")
    assert initiate.status_code == 302, (
        f"initiate: {initiate.status_code}: {initiate.text[:200]}"
    )
    state = parse_qs(urlsplit(initiate.headers["location"]).query).get("state", [None])[0]
    assert state, initiate.headers["location"]
    callback = client.get(
        "/auth/gitea/callback/", params={"code": "contract-code", "state": state}
    )
    assert callback.status_code == 302, (
        f"callback: {callback.status_code}: {callback.text[:300]}"
    )
    location = callback.headers["location"]
    query = parse_qs(urlsplit(location).query)
    assert "error_code" not in query, location
    assert "error_message" not in query, location
    return callback


def test_gitea_signup_avatar_key_is_bare(client: httpx.Client, ensure_instance):
    """Signup with ``image/png`` stores the bare key (Django ``base.py:185``).

    Runs on both backends. Pre-fix Rust stores
    ``user-<hex>-<hex>-user-avatar.png`` and the regex fails; post-fix
    both show ``<hex>-user-avatar.png``.
    """
    database_url = config.database_url()
    with _world(sync_enabled=False, content_type="image/png") as world:
        email, bucket = world["email"], world["bucket"]
        _drive_callback(client)

        avatar_hits = [
            r
            for r in world["avatars"].requests
            if urlsplit(r["path"]).path == "/avatar.png"
        ]
        assert avatar_hits, (
            "backend never downloaded the avatar; key assertions would be vacuous",
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
        assert row["attributes"]["name"] == "gitea-avatar.png", row
        assert row["attributes"]["type"] == "image/png", row
        assert row["attributes"]["size"] == len(AVATAR_PNG), row
        assert user["avatar_asset_id"] == row["id"], (user, rows)
        assert BARE_KEY.match(row["asset"]), row
        assert not row["asset"].startswith("user-"), row
        assert s3h.s3_head_key(bucket, row["asset"]) == 200, (
            "avatar S3 object present",
            row["asset"],
        )


@pytest.mark.parametrize(
    "content_type",
    ["image/png; charset=binary", "image/jpeg; charset=utf-8"],
)
def test_gitea_signup_avatar_parametrised_content_type_rejected(
    client: httpx.Client, ensure_instance, content_type
):
    """A parametrised Content-Type is rejected (Django exact match).

    Runs on both backends. Pre-fix Rust splits on ``;`` and uploads (a
    ``USER_AVATAR`` row exists); Django and post-fix Rust create no row
    and keep the provider URL.
    """
    database_url = config.database_url()
    with _world(sync_enabled=False, content_type=content_type) as world:
        email = world["email"]
        _drive_callback(client)

        avatar_hits = [
            r
            for r in world["avatars"].requests
            if urlsplit(r["path"]).path == "/avatar.png"
        ]
        assert avatar_hits, (
            "backend never downloaded the avatar; rejection would be vacuous",
            world["avatars"].requests,
        )

        user = db.fetchone(
            database_url,
            "SELECT id, avatar, avatar_asset_id FROM users WHERE email = %s",
            (email,),
        )
        assert user is not None, "callback created the user"
        assert user["avatar"] == world["avatar_url"], user
        assert user["avatar_asset_id"] is None, user
        rows = db.fetchall(
            database_url,
            "SELECT id FROM file_assets WHERE user_id = %s AND entity_type = 'USER_AVATAR'",
            (user["id"],),
        )
        assert rows == [], (
            f"parametrised {content_type!r} must not create a FileAsset, got {rows}"
        )


def test_gitea_sync_soft_deletes_on_answered_delete(client: httpx.Client, ensure_instance):
    """Sync replaces the avatar: old asset soft-deletes (Django gate).

    Runs on both backends. Signup uploads asset #1, sync's delete removes
    its S3 object and soft-deletes the row, and the re-upload lands
    asset #2. The 403 variant of this gate is pinned by the kernel-level
    ``gitea_kernel_delete_old_avatar_on_403_soft_deletes`` test (a live
    S3 403 cannot be injected per-test without rebooting the backend);
    this HTTP test pins the wire path both backends share.
    """
    database_url = config.database_url()
    with _world(sync_enabled=True, content_type="image/png") as world:
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
            " WHERE user_id = %s ORDER BY deleted_at NULLS LAST",
            (user["id"],),
        )
        assert len(rows) == 2, ("expected the signup asset + its sync replacement", rows)
        old, new = rows
        assert old["deleted_at"] is not None, rows
        assert new["deleted_at"] is None, rows
        assert user["avatar_asset_id"] == new["id"], (user, rows)
        assert user["avatar"] == "", user
        assert user["avatar_asset_id"] == new["id"], (user, rows)
        assert s3h.s3_head_key(bucket, old["asset"]) == 404, (
            "old S3 object deleted",
            old["asset"],
        )
        assert s3h.s3_head_key(bucket, new["asset"]) == 200, (
            "new S3 object present",
            new["asset"],
        )
