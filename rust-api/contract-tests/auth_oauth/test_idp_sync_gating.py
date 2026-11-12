# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract suite: IDP sync gating on the gitlab/gitea callbacks (PIDASHCONV-1322).

Django runs ``sync_user_data`` only for NEWLY created OAuth users
(``adapter/base.py:299,345``: ``is_signup = bool(user)`` is inverted, the
gate is ``check_sync_enabled() and not is_signup``; PB-1024). The Rust
gitlab port ran sync for EXISTING users on every login and never for new
ones; the gitea port gated it on a dead ``!is_signup`` inside the
existing-user arm, so it never ran at all.

Unlike github/google (hardcoded ``https://`` URLs — hermetically
untestable, see "Known oracle limits" in ``test_permissions.py``), the
gitlab/gitea provider hosts are DB-configurable (``GITLAB_HOST`` /
``GITEA_HOST``, registry ``source=db``, ``http`` allowed), so these tests
drive the REAL callback success path on both backends against a local stub
provider: token + userinfo + avatar endpoints on 127.0.0.1, provider config
via ``instance_configurations`` rows (saved/restored around each test),
and the callback ``state`` forged into ``sessions`` with
``CONTRACT_SECRET_KEY`` (as in ``test_github_side_effects.py``).

- GitLab, ``ENABLE_GITLAB_SYNC=1``, EXISTING user with custom names: Django
  skips sync, so the row is unchanged apart from login stamps; pre-fix Rust
  overwrote first/last/display/avatar. The stub avatar endpoint must never
  be hit (no creation download, no sync download).
- Gitea, ``ENABLE_GITEA_SYNC=1``, NEW user: Django runs sync, so the avatar
  is downloaded TWICE (creation upload, then sync's delete + re-upload);
  pre-fix Rust downloads once. Gitea ``user_data`` carries no
  ``display_name`` key (``gitea.py:149-173``), so without S3 the final row
  is identical either way — the download count is the sync signal, plus
  row sanity (created, local-part display name, avatar URL fallback).

The stub token response deliberately omits ``expires_in``: Django reads
``created_at + expires_in`` via ``fromtimestamp`` and the Rust gitlab port
maps a truthy ``expires_in`` with missing ``created_at`` to the generic
500 (both sides agree, but neither is this test's subject).
"""

import contextlib
import json
import os
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402

# 1x1 transparent PNG. Neither backend validates image content (only the
# Content-Type/size guards), but valid bytes keep the download path honest.
AVATAR_PNG = (
    b"\x89PNG\r\n\x1a\n"
    b"\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89"
    b"\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4"
    b"\x00\x00\x00\x00IEND\xaeB`\x82"
)

TOKEN_RESPONSE = {"access_token": "stub-access-token-1322", "token_type": "Bearer"}


class ProviderStub:
    """Minimal OAuth provider: token + userinfo + avatar, with a hit counter.

    ``userinfo`` may be assigned after construction (the avatar URL embeds
    :attr:`url`, known only once bound). All counters are lock-guarded;
    the server thread is daemonic and closed explicitly.
    """

    def __init__(self, *, token_paths, userinfo_path, userinfo):
        self.token_paths = set(token_paths)
        self.userinfo_path = userinfo_path
        self.userinfo = userinfo
        self._lock = threading.Lock()
        self._avatar_hits = 0
        stub = self

        class Handler(BaseHTTPRequestHandler):
            def _send(self, status, body, content_type):
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                length = int(self.headers.get("Content-Length") or 0)
                if length:
                    self.rfile.read(length)
                path = urlsplit(self.path).path
                if path in stub.token_paths:
                    self._send(200, json.dumps(TOKEN_RESPONSE).encode(), "application/json")
                else:
                    self._send(404, b"no such stub endpoint", "text/plain")

            def do_GET(self):
                path = urlsplit(self.path).path
                if path == stub.userinfo_path:
                    self._send(200, json.dumps(stub.userinfo).encode(), "application/json")
                elif path == "/avatar.png":
                    with stub._lock:
                        stub._avatar_hits += 1
                    self._send(200, AVATAR_PNG, "image/png")
                else:
                    self._send(404, b"no such stub endpoint", "text/plain")

            def log_message(self, *args):
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self):
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def avatar_count(self):
        with self._lock:
            return self._avatar_hits

    def close(self):
        self._server.shutdown()
        self._thread.join(timeout=10)
        self._server.server_close()


@contextlib.contextmanager
def provider_config(database_url, rows):
    """Set ``instance_configurations`` rows (db-sourced on both backends),
    restoring whatever was there before. Cleanup runs even on failure so
    the suite-wide ``oauth_unconfigured`` pin never sees leftovers."""
    keys = list(rows)
    saved = db.fetchall(
        database_url,
        "SELECT id, key, value, category, is_encrypted"
        " FROM instance_configurations WHERE key = ANY(%s)",
        (keys,),
    )
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (keys,),
    )
    for key, value in rows.items():
        db.execute(
            database_url,
            "INSERT INTO instance_configurations"
            " (id, key, value, category, is_encrypted, created_at, updated_at)"
            " VALUES (%s, %s, %s, 'general', false, now(), now())",
            (str(uuid.uuid4()), key, value),
        )
    try:
        yield
    finally:
        db.execute(
            database_url,
            "DELETE FROM instance_configurations WHERE key = ANY(%s)",
            (keys,),
        )
        for row in saved:
            db.execute(
                database_url,
                "INSERT INTO instance_configurations"
                " (id, key, value, category, is_encrypted, created_at, updated_at)"
                " VALUES (%s, %s, %s, %s, %s, now(), now())",
                (row["id"], row["key"], row["value"], row["category"], row["is_encrypted"]),
            )


def _forge_oauth_session(database_url, state):
    """Forge a ``sessions`` row carrying an OAuth ``state`` (the fixed
    ``CONTRACT_SECRET_KEY`` signs it exactly like Django). Returns the
    session key; the caller deletes the row."""
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    payload = {"host": "http://oauth-state.test", "next_path": "", "state": state}
    session_key = uuid.uuid4().hex
    db.execute(
        database_url,
        "INSERT INTO sessions (session_key, session_data, expire_date)"
        " VALUES (%s, %s, now() + make_interval(days => 7))",
        (session_key, forge_session_data(payload, secret)),
    )
    return session_key


def _callback(base_url, path, state):
    """GET one OAuth callback with matching session state + code."""
    database_url = config.database_url()
    session_key = _forge_oauth_session(database_url, state)
    try:
        authed = httpx.Client(
            base_url=base_url,
            cookies={SESSION_COOKIE: session_key},
            timeout=30,
            follow_redirects=False,
        )
        try:
            return authed.get(path, params={"code": "stub-code-1322", "state": state})
        finally:
            authed.close()
    finally:
        db.execute(
            database_url,
            "DELETE FROM sessions WHERE session_key = %s",
            (session_key,),
        )


def _delete_user_tree(store, user_id):
    """Delete a users row plus every row the OAuth callbacks create for it
    (profile, notification preferences, avatar assets, linked account)."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM user_notification_preferences WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM profiles WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM file_assets WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM accounts WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM users WHERE id = %s", (user_id,))
        conn.commit()


def _assert_success(resp):
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    query = parse_qs(urlsplit(resp.headers["location"]).query)
    assert "error_code" not in query, resp.headers["location"]


def test_gitlab_sync_enabled_leaves_existing_user_untouched(base_url: str, ensure_instance):
    """Existing-user gitlab login with sync on: names/display/avatar unchanged.

    Fails on pre-fix Rust (sync overwrote the row + downloaded the avatar);
    passes on Django and fixed Rust.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gl1322-{tag}@example.test"
    user = store.make_user(email)
    old_avatar = "https://example.test/old-avatar-1322.png"
    db.execute(
        database_url,
        "UPDATE users SET first_name = 'X', last_name = 'Y',"
        " display_name = 'Custom Name', avatar = %s WHERE id = %s",
        (old_avatar, user["id"]),
    )
    stub = ProviderStub(
        token_paths={"/oauth/token"},
        userinfo_path="/api/v4/user",
        userinfo={
            "id": f"gl-1322-{tag}",
            "email": email,
            "name": "Provider First",
            "family_name": "Provider Last",
            "avatar_url": None,  # filled in once bound
        },
    )
    try:
        stub.userinfo["avatar_url"] = stub.url + "/avatar.png"
        rows = {
            "GITLAB_CLIENT_ID": "stub-gitlab-id",
            "GITLAB_CLIENT_SECRET": "stub-gitlab-secret",
            "GITLAB_HOST": stub.url,
            "ENABLE_GITLAB_SYNC": "1",
        }
        with provider_config(database_url, rows):
            resp = _callback(base_url, "/auth/gitlab/callback/", f"gl1322-state-{tag}")
    finally:
        stub.close()
    try:
        _assert_success(resp)
        row = db.fetchone(
            database_url,
            "SELECT first_name, last_name, display_name, avatar,"
            " last_login_medium, last_login_time FROM users WHERE id = %s",
            (user["id"],),
        )
        # The login itself must have run (guards against a false pass
        # where the callback errored before touching the row).
        assert row["last_login_medium"] == "gitlab", row
        assert row["last_login_time"] is not None, row
        # ... while sync stayed off for this existing user.
        assert row["first_name"] == "X", row
        assert row["last_name"] == "Y", row
        assert row["display_name"] == "Custom Name", row
        assert row["avatar"] == old_avatar, row
        assert stub.avatar_count() == 0, "sync downloaded the avatar for an existing user"
    finally:
        _delete_user_tree(store, user["id"])


def test_gitea_sync_enabled_runs_for_new_user(base_url: str, ensure_instance):
    """New-user gitea signup with sync on: sync side effects present.

    Django downloads the avatar twice (creation upload, then sync's delete
    + re-upload); pre-fix Rust downloads once (sync never runs). The final
    row is asserted for sanity (created, local-part display name, avatar
    URL fallback when the S3 upload fails as in the contract envs).
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    local = f"gt1322-{tag}"
    email = f"{local}@example.test"
    stub = ProviderStub(
        token_paths={"/login/oauth/access_token"},
        userinfo_path="/api/v1/user",
        userinfo={
            "id": 4242,
            "email": email,
            "login": local,
            "full_name": "Provider New",
            "avatar_url": None,  # filled in once bound
        },
    )
    try:
        avatar_url = stub.url + "/avatar.png"
        stub.userinfo["avatar_url"] = avatar_url
        rows = {
            "GITEA_CLIENT_ID": "stub-gitea-id",
            "GITEA_CLIENT_SECRET": "stub-gitea-secret",
            "GITEA_HOST": stub.url,
            "ENABLE_GITEA_SYNC": "1",
        }
        with provider_config(database_url, rows):
            resp = _callback(base_url, "/auth/gitea/callback/", f"gt1322-state-{tag}")
    finally:
        stub.close()
    user_id = None
    try:
        _assert_success(resp)
        row = db.fetchone(
            database_url,
            "SELECT id, first_name, last_name, display_name, avatar,"
            " last_login_medium FROM users WHERE email = %s",
            (email,),
        )
        assert row is not None, "gitea signup created no users row"
        user_id = row["id"]
        assert row["first_name"] == "Provider New", row
        assert row["last_name"] == "", row
        assert row["display_name"] == local, row
        assert row["avatar"] == avatar_url, row
        assert row["last_login_medium"] == "gitea", row
        assert stub.avatar_count() == 2, (
            f"expected creation + sync downloads, got {stub.avatar_count()}"
        )
    finally:
        if user_id is not None:
            _delete_user_tree(store, user_id)
