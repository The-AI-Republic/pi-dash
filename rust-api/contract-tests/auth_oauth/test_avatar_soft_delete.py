"""Contract suite: OAuth avatar replacement soft-deletes the old asset (PIDASHCONV-1321).

Django's ``delete_old_avatar`` (``authentication/adapter/base.py:236-254``)
soft-deletes the old ``FileAsset`` (``deleted_at`` stamp + save +
``soft_delete_related_objects.delay``) after removing its S3 object, then
clears ``users.avatar_asset_id``/``avatar``. The Rust google and gitea
copies instead hard-``DELETE`` the row while the user's FK still points
at it; the deferred FK check fires at the statement end in autocommit, so
the DELETE always fails with 23503 and the code returns early — the S3
object is gone but the row stays live and still referenced.

Harness choice: gitea accepts a host override, so this module points
``GITEA_HOST`` at a loopback stub (the PIDASHCONV-1089 seam) serving the
token + userinfo endpoints, enables ``ENABLE_GITEA_SYNC``, and drives the
full app-gitea signup callback over plain HTTP on both backends. A second
stub serves the avatar image bytes (bound first so its URL can be
embedded in the userinfo payload). The signup branch uploads avatar asset
#1; sync's ``delete_old_avatar`` must soft-delete it and the re-upload
lands asset #2. Google hardcodes its endpoints on both backends (sealed),
so its copy is pinned by the kernel-level harness (``google_kernel_*``
live tests in ``oauth_google.rs`` driving ``delete_old_avatar`` directly).

S3: this is the first HTTP suite doing live S3 I/O. Both backends resolve
the endpoint from ``AWS_S3_ENDPOINT_URL`` with ``USE_MINIO`` unset (MinIO
mode signs the app's own host on both sides, so it cannot reach a test
bucket); the Rust-matrix workflow provisions LocalStack for the
``auth_oauth`` suite only, and the test creates the contract bucket
itself. No test here contacts a real provider.

The ``GITEA_*``/``ENABLE_GITEA_SYNC`` ``instance_configurations`` rows are
inserted function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests run
serially; no fixture can observe the rows mid-test). Teardown lives in the
fixture (always runs, even when the callback or an early assertion fails):
leaking ``ENABLE_GITEA_SYNC`` would silently flip sync on for every later
gitea-callback test in the module and poison the suite.
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
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402
from _harness import s3 as s3h  # noqa: E402

CONFIG_KEYS = (
    "GITEA_CLIENT_ID",
    "GITEA_CLIENT_SECRET",
    "GITEA_HOST",
    "ENABLE_GITEA_SYNC",
)
SESSION_HOST = "http://oauth-state.test"

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
            cur.execute("DELETE FROM sessions WHERE session_key LIKE 'av1321-%'")
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

    Sibling gitea-callback tests assume the S3 upload fails in the
    contract envs (no bucket ⇒ NoSuchBucket ⇒ provider-URL fallback);
    a leaked bucket would flip their uploads to success and break them
    (PIDASHCONV-1322's gating test asserts the fallback row).
    """
    try:
        s3h.s3_call("DELETE", f"{s3h.s3_endpoint()}/{bucket}")
    except Exception:
        pass


@pytest.fixture()
def gitea_avatar_world():
    """Fresh signup email behind a stub Gitea + avatar stub, sync enabled.

    Yields the world with both stubs running and the config rows in place;
    teardown (always runs) removes the S3 objects, the bucket itself (so
    sibling tests keep their no-bucket upload-failure assumption), the
    config rows, then the world. The avatar stub binds first: its URL is
    embedded in the userinfo payload the Gitea stub serves.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"av1321-{tag}@example.test"
    bucket = s3h.contract_bucket()
    s3h.s3_setup_bucket(bucket)
    world = {"tag": tag, "email": email, "bucket": bucket}
    with RoutingStub({("GET", "/avatar.png"): (200, AVATAR_PNG, "image/png")}) as avatars:
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
                    "login": f"av1321-{tag}",
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
            # Reap before sow: a crashed earlier run must not leak config
            # rows into this one (or, via teardown, into sibling modules).
            _delete_config(database_url)
            try:
                store.make_config(
                    "GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION"
                )
                store.make_config(
                    "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
                )
                store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
                store.make_config("ENABLE_GITEA_SYNC", "1", category="AUTHENTICATION")
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


def test_app_gitea_signup_sync_soft_deletes_replaced_avatar(
    base_url: str, gitea_avatar_world
):
    """Full app-gitea signup with sync on: the replaced avatar soft-deletes.

    Runs against both backends. The signup branch uploads asset #1, sync
    deletes its S3 object, soft-deletes the row and clears the FK, and the
    re-upload lands asset #2. Pre-fix Rust hard-deletes (23503) and returns
    early, so the first row stays live and the ``deleted_at`` assertion
    fails; post-fix both backends show one live + one soft-deleted row,
    the user pointing at the live one, the old S3 object gone.
    """
    database_url = config.database_url()
    world = gitea_avatar_world
    email, bucket = world["email"], world["bucket"]
    state = f"av1321-state-{world['tag']}"
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    session_key = f"av1321-{uuid.uuid4().hex}"
    db.execute(
        database_url,
        "INSERT INTO sessions (session_key, session_data, expire_date)"
        " VALUES (%s, %s, now() + make_interval(days => 7))",
        (
            session_key,
            forge_session_data({"host": SESSION_HOST, "state": state}, secret),
        ),
    )
    try:
        authed = httpx.Client(
            base_url=base_url,
            cookies={SESSION_COOKIE: session_key},
            timeout=30,
            follow_redirects=False,
        )
        try:
            resp = authed.get(
                "/auth/gitea/callback/",
                params={"code": "contract-stub-code", "state": state},
            )
        finally:
            authed.close()
    finally:
        db.execute(
            database_url, "DELETE FROM sessions WHERE session_key = %s", (session_key,)
        )

    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    )
    location = resp.headers["location"]
    assert location.startswith(SESSION_HOST), location
    assert "error_code" not in parse_qs(urlsplit(location).query), location

    stub = world["stub"]
    hits = {(r["method"], urlsplit(r["path"]).path) for r in stub.requests}
    assert ("POST", "/login/oauth/access_token") in hits, stub.requests
    assert ("GET", "/api/v1/user") in hits, stub.requests
    avatar_hits = [
        r
        for r in world["avatars"].requests
        if urlsplit(r["path"]).path == "/avatar.png"
    ]
    assert avatar_hits, (
        "backend never downloaded the avatar; sync assertions would be vacuous",
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
