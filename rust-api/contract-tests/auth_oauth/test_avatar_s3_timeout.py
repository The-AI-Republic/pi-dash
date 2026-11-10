# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract suite: OAuth avatar S3 PUT/HEAD/DELETE are bounded (PIDASHCONV-1728).

Django's ``S3Storage`` (``settings/storage.py:43-76``) builds a boto3 client with
no timeout ``Config``, so botocore's defaults apply (60s connect / 60s read,
plus retries). ``adapter/base.py:198-215`` (upload + metadata) and
``:236-254`` (delete) both swallow any failure — upload returns ``None`` and
the provider URL is kept, delete logs and continues.

The Rust port historically built the avatar S3 client with
``reqwest::Client::new()`` (``oauth_github.rs:1798``,
``oauth_gitlab.rs:1891``, ``oauth_gitea.rs:2060``) or an unbounded
``Client::builder().build()`` (``oauth_google.rs:2130``). Only the provider
image download set a 10s per-request timeout. On a stalled S3/MinIO the
server-side PUT at ``mod.rs:268``, HEAD at ``:334`` and DELETE at ``:488``
(and the Gitea copies at ``:1283/:1310/:1442`` and Google copies at
``:1154-1460``) hang the OAuth callback indefinitely, so sign-in never
completes. On the MinIO branch both sides sign the public request host
(``S3Storage(request)``, ``is_server=False``), so a hung reverse proxy is
enough to stall every sign-up.

Fix: the shared ``auth_oauth/mod.rs::avatar_s3_client()`` builds a client
with ``connect_timeout(60s)`` + ``read_timeout(60s)`` (the
``jobs/src/tasks_cleanup/expiry.rs:205-210`` precedent, PIDASHCONV-1291).
All four providers pass that client to the PUT/HEAD/DELETE helpers —
GitHub/GitLab share ``mod.rs``, Gitea and Google keep their own copies but
route them through the same bounded client. The provider download keeps its
tighter 10s per-request timeout; token/userinfo stay unbounded like
Django's ``requests``.

Harness: this module has two layers.

1. ``test_avatar_s3_clients_have_bounded_timeouts`` — the fast static pin.
   Reads the shipped Rust sources and asserts the helper exists with the
   60/60 pair and that every avatar call site wires it (no unbounded
   ``Client::new()`` on the avatar path). Fails on the pre-fix tree
   (``Client::new()`` / unbounded builder, no helper), passes on the fixed
   tree. Runs on every suite invocation without a stall endpoint, on both
   backends (the source layout is the same).

2. ``test_avatar_s3_stall_falls_back`` — the behavioral stall oracle. With
   a stalled S3 endpoint (a BlackHole listener at ``STALL_S3_PORT``, default
   4599, like ``tasks_cleanup/test_s3_stall.py``) and a fake GitLab provider,
   the OAuth signup callback must complete within a bounded time, the user's
   ``avatar`` must be the provider URL, and no ``USER_AVATAR`` ``FileAsset``
   row may exist. Before the fix the Rust callback hangs (test times out at
   its bound); after the fix it falls back at ~60s. Django passes before and
   after (boto3 60s). This test needs the backend under test booted with
   its S3 endpoint pointed at the BlackHole (``AWS_S3_ENDPOINT_URL`` +
   ``USE_MINIO=1``, stall job) or booted so the S3 PUT hits the BlackHole
   via the public host — it skips with a clear reason when no stall is
   observed, so a non-stall CI leg does not hang.

No real provider is contacted: the GitLab token/userinfo stub is loopback
(the PIDASHCONV-1360 host-override seam); the avatar stub is loopback too,
bound before the userinfo payload so its URL can be embedded.
"""

import base64
import os
import re
import sys
import time
import uuid
from contextlib import contextmanager
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db, stall  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402
from _harness import s3 as s3h  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
RUST_AUTH = REPO_ROOT / "rust-api" / "crates" / "api" / "src" / "auth_oauth"
MOD_RS = RUST_AUTH / "mod.rs"
GITHUB_RS = RUST_AUTH / "oauth_github.rs"
GITLAB_RS = RUST_AUTH / "oauth_gitlab.rs"
GITEA_RS = RUST_AUTH / "oauth_gitea.rs"
GOOGLE_RS = RUST_AUTH / "oauth_google.rs"

CONFIG_KEYS = (
    "GITLAB_CLIENT_ID",
    "GITLAB_CLIENT_SECRET",
    "GITLAB_HOST",
    "ENABLE_GITLAB_SYNC",
)

AVATAR_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


def test_avatar_s3_clients_have_bounded_timeouts():
    """The avatar S3 client is bounded 60s/60s and wired from all providers.

    Fails on the pre-fix tree (no helper, unbounded ``Client::new()``);
    passes after the fix. The helper mirrors the jobs expiry
    ``build_s3_client`` precedent exactly.
    """
    mod_rs = MOD_RS.read_text()
    assert "fn avatar_s3_client" in mod_rs, (
        "mod.rs must define avatar_s3_client() (PIDASHCONV-1728)"
    )
    assert "connect_timeout(std::time::Duration::from_secs(60))" in mod_rs, (
        "avatar_s3_client must set connect_timeout(60s) like botocore"
    )
    assert "read_timeout(std::time::Duration::from_secs(60))" in mod_rs, (
        "avatar_s3_client must set read_timeout(60s) like botocore"
    )

    # Every provider's avatar construction must go through the helper.
    github = GITHUB_RS.read_text()
    assert "avatar_s3_client()" in github, "github must wire avatar_s3_client()"
    # The old unbounded avatar client must be gone from the callback path.
    # (Token/userinfo clients elsewhere in the files may still use
    # Client::new() / builder — Django's provider calls are unbounded too.)
    # The callback's avatar line was 1798: assert it no longer is Client::new().
    # We check the specific pattern: the avatar client assignment.
    assert "let avatar_http = super::avatar_s3_client()" in github, (
        "github avatar_http must be avatar_s3_client() (was Client::new() at 1798)"
    )

    gitlab = GITLAB_RS.read_text()
    assert "avatar_s3_client()" in gitlab, "gitlab must wire avatar_s3_client()"
    assert "let client = super::avatar_s3_client()" in gitlab, (
        "gitlab avatar client must be avatar_s3_client() (was Client::new() at 1891)"
    )

    gitea = GITEA_RS.read_text()
    assert "avatar_s3_client()" in gitea, "gitea must wire avatar_s3_client()"
    assert "s3_client" in gitea, "gitea must pass s3_client to avatar helpers"
    # The three avatar helpers must take &s3_client, not the token &client.
    assert "&s3_client," in gitea, "gitea avatar calls must use &s3_client"
    # Gitea's avatar copies (:1283 PUT, :1310 HEAD, :1442 DELETE) are executed
    # with that bounded client — the download keeps its 10s request timeout.
    assert "Duration::from_secs(10)" in gitea, (
        "gitea provider download must keep its 10s per-request timeout"
    )

    google = GOOGLE_RS.read_text()
    assert "avatar_s3_client()" in google, "google must wire avatar_s3_client()"
    assert "s3_client" in google, "google must pass s3_client to avatar helpers"
    assert "&s3_client," in google, "google avatar calls must use &s3_client"
    assert "Duration::from_secs(10)" in google, (
        "google provider download must keep its 10s per-request timeout"
    )

    # The shared helpers (mod.rs PUT :268, HEAD :334, DELETE :488) must not
    # add per-request timeouts that widen or narrow the S3 bounds — they
    # inherit the client's 60s/60s. The only per-request timeout in the
    # shared download path is the provider GET at 10s.
    for needle in (".put(signed.url)", ".head(signed.url)", ".delete(signed.url)"):
        assert needle in mod_rs, f"mod.rs must contain {needle}"
    # mod.rs download still has the 10s guard (not widened to 60s).
    assert ".timeout(std::time::Duration::from_secs(10))" in mod_rs, (
        "shared download must keep .timeout(10s)"
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


@contextmanager
def _gitlab_world(stall_s3_port=None):
    """Fake GitLab + avatar stubs, GitLab config in the DB.

    Yields a world with the stubs running and config rows in place; the
    backend under test must resolve its S3 PUT/HEAD/DELETE to ``stall_s3_port``
    (BlackHole) for the stall to be observed — see module docstring.
    Teardown always runs so the boot-time hermeticity holds for siblings.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gl1728-{tag}@example.test"
    world = {"tag": tag, "email": email}
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
                    "name": "Stall Avatar User",
                    "username": f"gl1728-{tag}",
                    "avatar_url": f"{avatars.url}/avatar.png",
                },
            ),
        }
        with RoutingStub(gitlab_routes) as stub:
            _delete_config(database_url)
            try:
                store.make_config(
                    "GITLAB_CLIENT_ID", "contract-id", category="AUTHENTICATION"
                )
                store.make_config(
                    "GITLAB_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
                )
                store.make_config("GITLAB_HOST", stub.url, category="AUTHENTICATION")
                # Sync off: only the signup PUT/HEAD path stalls.
            except Exception:
                _delete_config(database_url)
                raise
            try:
                world["avaters"] = avatars
                world["stub"] = stub
                world["avatar_url"] = f"{avatars.url}/avatar.png"
                yield world
            finally:
                _delete_config(database_url)
                _delete_world(store, email)


def _drive_gitlab_callback(client: httpx.Client, timeout=120):
    initiate = client.get("/auth/gitlab/", timeout=timeout)
    assert initiate.status_code == 302, (
        f"initiate: {initiate.status_code}: {initiate.text[:200]}"
    )
    state = parse_qs(urlsplit(initiate.headers["location"]).query).get("state", [None])[0]
    assert state, initiate.headers["location"]
    # The callback must complete within a bounded time even when S3 stalls.
    # We use a httpx timeout of 120s (botocore 60s + headroom) so a hung
    # backend fails the test instead of hanging the suite.
    return client.get(
        "/auth/gitlab/callback/", params={"code": "contract-code", "state": state}, timeout=timeout
    )


def test_avatar_s3_stall_falls_back(client: httpx.Client, ensure_instance):
    """A stalled S3 falls back to the provider URL within a bounded time.

    Stall environment: the backend under test boots with its S3 endpoint
    pointed at the BlackHole (``STALL_S3_PORT``/4599, ``USE_MINIO=1``) so the
    server-side PUT hits the listener. The callback must complete, the user
    must carry the provider URL, and no ``USER_AVATAR`` row may exist.

    Before the fix, Rust hangs (the callback never returns, this test hits
    its 120s httpx timeout and fails); after the fix it falls back at ~60s
    on both backends. When the S3 endpoint was never stalled (hits == 0),
    the test fails with an explicit reason instead of passing vacuously —
    the stall must actually be observed.
    """
    database_url = config.database_url()
    # The stall port the backend's S3 endpoint points at (fixed before boot).
    port = stall.stall_port()
    # The avatar download stub still serves the provider bytes; the S3 side
    # stalls. The GitLab provider stub lives on its own loopback port, so
    # token/userinfo are never stalled — only the S3 PUT/HEAD/DELETE.
    with stall.black_hole(port=port) as listener:
        with _gitlab_world() as world:
            email = world["email"]
            # Time the callback; it must be bounded even on a BlackHole.
            started = time.monotonic()
            resp = _drive_gitlab_callback(client, timeout=120)
            elapsed = time.monotonic() - started
            # The provider avatar must have been downloaded for the S3
            # attempt to be meaningful.
            avatar_hits = [
                r
                for r in world["avaters"].requests
                if urlsplit(r["path"]).path == "/avatar.png"
            ]
            assert avatar_hits, (
                "backend never downloaded the avatar; S3 stall would be vacuous",
                world["avaters"].requests,
            )
            # Callback completed within the expected bound (botocore 60s
            # read + headroom, well under the 120s httpx guard).
            assert resp.status_code == 302, (
                f"callback: {resp.status_code}: {resp.text[:300]}; "
                f"elapsed {elapsed:.1f}s"
            )
            location = resp.headers.get("location", "")
            query = parse_qs(urlsplit(location).query)
            assert "error_code" not in query, location
            assert "error_message" not in query, location

    # After the BlackHole closes, S3 no longer stalls — assert the DB fell
    # back. The listener hits prove the S3 PUT actually dialed the stall.
    assert listener.hits >= 1, (
        "worker never dialed the stall endpoint: the backend under test was "
        f"not booted with its S3 endpoint at http://127.0.0.1:{port} (USE_MINIO=1 "
        "or AWS_S3_ENDPOINT_URL). Re-run in the stall job (see module docstring)."
    )
    user = db.fetchone(
        database_url,
        "SELECT id, avatar, avatar_asset_id FROM users WHERE email = %s",
        (email,),
    )
    assert user is not None, "callback created the user despite the S3 stall"
    assert user["avatar"] == world["avatar_url"], (
        f"avatar must fall back to the provider URL, got {user['avatar']!r}"
    )
    rows = db.fetchall(
        database_url,
        "SELECT id FROM file_assets WHERE user_id = %s AND entity_type = 'USER_AVATAR'",
        (user["id"],),
    )
    assert rows == [], (
        f"no USER_AVATAR FileAsset row may exist after a stalled S3, got {rows}"
    )
