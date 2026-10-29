# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Returning OAuth logins preserve accounts.created_at + metadata (PIDASHCONV-1363).

Django's shared ``OauthAdapter.create_update_account``
(``authentication/adapter/oauth.py``) loads the existing row, assigns the
six token fields + ``last_connected_at``, and ``.save()``s it back, so
``created_at`` (``auto_now_add``) and ``metadata`` survive every login.
The Rust kernel rewrites both columns from its ``existing`` argument and
falls back to now/``{}`` on ``None``; the github and gitea callers passed
``None`` on every returning login, wiping the link date and metadata
(which the private cloud uses, e.g. ``scopes_requested``).

Harness split (the issue's Done-when asks for both providers over HTTP,
but one arm is sealed):

1. ``test_gitea_returning_login_preserves_created_at_and_metadata``
   drives a REAL gitea login on both backends: ``GITEA_HOST`` is
   DB-configured and both sides accept an ``http://`` stub (Django
   ``gitea.py`` scheme check, Rust ``normalize_gitea_host``), so an
   in-process stub serves the token + userinfo + emails endpoints and
   ``instance_configurations`` rows point the backend at it. No server
   env, no production change. FAILS on pre-fix Rust (``created_at`` reset
   to now, ``metadata`` to ``{}``), passes on Django and fixed Rust.
2. Github-over-HTTP is hermetically sealed on both backends (hardcoded
   ``https://`` URLs; the Rust ``reqwest`` client has no proxy support),
   per PIDASHCONV-1083/1084. The github regression net is the live kernel
   test ``github_kernel_returning_login_preserves_account_history`` in
   ``oauth_github.rs`` (drives ``complete_login_or_signup`` directly;
   fails pre-fix) plus the static pin below.
3. ``test_upsert_passes_prior_account[github|gitea]`` pins the shipped
   call sites: the ``create_update_account`` call must take the loaded
   ``existing`` row, not ``None``. FAILS pre-fix on both files.
4. ``test_django_adapter_preserves_account_history`` pins the oracle:
   if ``adapter/oauth.py`` ever assigns ``created_at``/``metadata`` on
   the update side, the premise changed and every provider port must be
   revisited. (Django's adapter is provider-agnostic, so the live gitea
   run on Django is the github oracle too.)

The GITEA_* rows exist only inside the live test (inserted after
asserting none pre-exist, deleted in ``finally``), so the
session-scoped ``oauth_unconfigured`` fixture never sees them.
"""

import json
import os
import re
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
GITHUB_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_github.rs"
GITEA_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_gitea.rs"
ADAPTER_PY = REPO_ROOT / "apps/api/pi_dash/authentication/adapter/oauth.py"

OLD_CREATED = "2020-01-01T00:00:00+00:00"
OLD_METADATA = {"k": 1}
GITEA_KEYS = ("GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET", "GITEA_HOST")


class _StubHandler(BaseHTTPRequestHandler):
    server_version = "GiteaStub/1363"

    def _send_json(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):  # noqa: N802 (stdlib handler name)
        parsed = urlparse(self.path)
        length = int(self.headers.get("Content-Length", 0) or 0)
        raw = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        if parsed.path != "/login/oauth/access_token":
            self._send_json({"error": "not found"}, status=404)
            return
        code = "nocode"
        for part in raw.split("&"):
            if part.startswith("code="):
                code = part[5:][:32]
        self._send_json(
            {
                "access_token": f"stub-access-{code}",
                "token_type": "bearer",
                "expires_in": 3600,
                "refresh_token": f"stub-refresh-{code}",
                "id_token": "",
            }
        )

    def do_GET(self):  # noqa: N802 (stdlib handler name)
        parsed = urlparse(self.path)
        state = self.server.stub_state
        if parsed.path == "/api/v1/user":
            self._send_json(
                {
                    "id": state["provider_id"],
                    "email": state["email"],
                    "full_name": "Gitea Contract User",
                    "login": "gitea-contract-user",
                    "avatar_url": "",
                }
            )
        elif parsed.path == "/api/v1/user/emails":
            self._send_json(
                [{"email": state["email"], "primary": True, "verified": True}]
            )
        else:
            self._send_json({"error": "not found"}, status=404)

    def log_message(self, *args):
        pass


@pytest.fixture()
def gitea_stub():
    """In-process fake Gitea provider. Yields ``(host_url, set_state)``."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), _StubHandler)
    server.stub_state = {}
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        port = server.server_address[1]
        yield f"http://127.0.0.1:{port}", server.stub_state.update
    finally:
        server.shutdown()
        thread.join(timeout=10)


def _forge_oauth_session(database_url, state):
    secret = os.environ["CONTRACT_SECRET_KEY"]
    payload = {"host": "http://oauth-state.test", "next_path": "", "state": state}
    session_key = uuid.uuid4().hex
    db.execute(
        database_url,
        "INSERT INTO sessions (session_key, session_data, expire_date)"
        " VALUES (%s, %s, now() + make_interval(days => 7))",
        (session_key, forge_session_data(payload, secret)),
    )
    return session_key


def test_gitea_returning_login_preserves_created_at_and_metadata(
    base_url: str, ensure_instance, gitea_stub
):
    """Returning gitea login: only the token fields move (both backends).

    Seeds an account with a 2020 ``created_at`` and non-empty metadata,
    logs in through the real callback against the stub provider, and
    asserts both values are byte-identical afterwards while the token
    fields and ``last_connected_at`` advance (proving the UPDATE ran).
    FAILS on pre-fix Rust.
    """
    database_url = config.database_url()
    stub_host, set_stub_state = gitea_stub
    tag = f"aup{uuid.uuid4().hex[:8]}"
    email = f"{tag}@example.test"
    provider_id = f"1363-{tag}"
    state = f"state1363-{tag}"
    preexisting = db.fetchall(
        database_url,
        "SELECT key FROM instance_configurations WHERE key = ANY(%s)",
        (list(GITEA_KEYS),),
    )
    assert not preexisting, (
        "GITEA_* configuration rows already present "
        f"{sorted(r['key'] for r in preexisting)}: this suite boots "
        "unconfigured (conftest.oauth_unconfigured); remove them first."
    )
    set_stub_state({"email": email, "provider_id": provider_id})
    store = Database(database_url)
    user = store.make_user(email, is_active=True)
    account_id = str(uuid.uuid4())
    db.execute(
        database_url,
        """INSERT INTO accounts
           (id, created_at, updated_at, user_id, provider_account_id, provider,
            access_token, access_token_expired_at, refresh_token,
            refresh_token_expired_at, last_connected_at, id_token, metadata)
           VALUES (%s, %s, now(), %s, %s, 'gitea', 'old-access', NULL,
                   'old-refresh', NULL, %s, '', %s)""",
        (
            account_id,
            OLD_CREATED,
            user["id"],
            provider_id,
            OLD_CREATED,
            json.dumps(OLD_METADATA),
        ),
    )
    for key, value in (
        ("GITEA_CLIENT_ID", "stub-client-id"),
        ("GITEA_CLIENT_SECRET", "stub-client-secret"),
        ("GITEA_HOST", stub_host),
    ):
        db.execute(
            database_url,
            """INSERT INTO instance_configurations
               (id, created_at, updated_at, key, value, category, is_encrypted,
                created_by_id, updated_by_id, deleted_at)
               VALUES (%s, now(), now(), %s, %s, 'authentication', false,
                       NULL, NULL, NULL)""",
            (str(uuid.uuid4()), key, value),
        )
    session_key = _forge_oauth_session(database_url, state)
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
                params={"code": f"stub-code-{tag}", "state": state},
            )
        finally:
            authed.close()
        assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
        assert resp.headers["location"].startswith("http://oauth-state.test/"), resp.headers["location"]
        row = db.fetchone(
            database_url,
            "SELECT created_at, metadata, access_token, refresh_token,"
            " last_connected_at FROM accounts WHERE id = %s",
            (account_id,),
        )
        assert row is not None, "accounts row vanished across the login"
        assert row["created_at"].isoformat().startswith("2020-01-01"), row["created_at"]
        assert row["metadata"] == OLD_METADATA, row["metadata"]
        assert row["access_token"].startswith("stub-access-stub-code-"), row["access_token"]
        assert row["refresh_token"].startswith("stub-refresh-stub-code-"), row["refresh_token"]
        assert row["last_connected_at"].isoformat() > "2020-01-02", row["last_connected_at"]
    finally:
        db.execute(
            database_url,
            "DELETE FROM sessions WHERE session_key = %s",
            (session_key,),
        )
        db.execute(
            database_url,
            "DELETE FROM instance_configurations WHERE key = ANY(%s)",
            (list(GITEA_KEYS),),
        )
        db.execute(
            database_url, "DELETE FROM accounts WHERE user_id = %s", (user["id"],)
        )
        db.execute(
            database_url, "DELETE FROM profiles WHERE user_id = %s", (user["id"],)
        )
        db.execute(database_url, "DELETE FROM users WHERE id = %s", (user["id"],))


def _upsert_call_block(path: Path, anchor: str) -> tuple[str, str]:
    """The ``create_update_account(...)`` call plus the code above it."""
    assert path.is_file(), f"harness: {path} moved — update test paths"
    text = path.read_text()
    hits = list(re.finditer(re.escape(anchor) + r"\(", text))
    assert len(hits) == 1, (
        f"harness: expected exactly one {anchor}( in {path.name},"
        f" found {len(hits)} — update the harness"
    )
    start = hits[0].start()
    await_at = text.find(".await", start)
    assert await_at != -1, "harness: could not delimit the call block"
    end = text.find("?;", await_at)
    assert end != -1, "harness: could not delimit the call block"
    return text[start : end + 2], text[max(0, start - 2500) : start]


@pytest.mark.parametrize(
    "source,anchor",
    [
        (GITHUB_RS, "account_queries::create_update_account"),
        # ORM-07 moved the account kernel to domain::identity; the gitea
        # call site references it by full path (github via its alias).
        (
            GITEA_RS,
            "pidash_services::domain::identity::account::create_update_account",
        ),
    ],
    ids=["github", "gitea"],
)
def test_upsert_passes_prior_account(source: Path, anchor: str):
    """The account upsert must carry the loaded row, not ``None`` (both files).

    Fails pre-fix (the call ends in ``None`` with no ``ExistingAccount``
    above it); passes once the caller SELECTs the prior
    ``(created_at, metadata)`` like ``oauth_google.rs``.
    """
    block, head = _upsert_call_block(source, anchor)
    call = block[: block.index(".await")]
    args = call[call.index("(") + 1 : call.rindex(")")]
    last = [arg.strip() for arg in args.split(",") if arg.strip()][-1]
    assert last == "existing", (
        f"{source.name}: {anchor} takes the loaded row (got final arg"
        f" {last!r}) — None resets created_at to now and metadata to {{}}"
    )
    assert "ExistingAccount" in head, f"{source.name}: no ExistingAccount above the call"
    assert 'SELECT \\"created_at\\", \\"metadata\\"' in head, (
        f"{source.name}: no prior-row SELECT above the call"
    )


def test_django_adapter_preserves_account_history():
    """Django pin: the shared update side touches no history columns."""
    assert ADAPTER_PY.is_file(), f"harness: {ADAPTER_PY} moved — update test paths"
    text = ADAPTER_PY.read_text()
    start = text.find("def create_update_account")
    assert start != -1, "harness: create_update_account not found in adapter/oauth.py"
    block = text[start : start + 3000]
    assert "Account.objects.create(" in block, "harness: sliced the method short"
    assert "account.save()" in block, "harness: sliced the method short"
    for field in (
        "access_token",
        "refresh_token",
        "access_token_expired_at",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
    ):
        assert f"account.{field} =" in block, f"harness: {field} assignment moved"
    assert "created_at" not in block, (
        "oracle changed: adapter/oauth.py now assigns created_at on the"
        " update side — revisit every provider port"
    )
    assert "metadata" not in block, (
        "oracle changed: adapter/oauth.py now assigns metadata on the"
        " update side — revisit every provider port"
    )
