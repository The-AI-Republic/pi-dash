# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract: GitLab callback with code but NO state param must reject (PIDASHCONV-1361).

Django (``views/app/gitlab.py:63-66``, ``views/space/gitlab.py:61-65``)
reads ``state = request.GET.get("state")`` — ``None`` when the param is
absent — and compares it against ``request.session.get("state", "")``.
``None != ""`` always mismatches, so a cookieless ``?code=X`` callback
stops before any provider traffic: the app answers the
``GITLAB_OAUTH_PROVIDER_ERROR`` (5121) redirect with no side effects,
and the space answers 500 (its ``base_host`` shadow bug) with no side
effects.

The Rust port (``oauth_gitlab.rs`` app ~717, space ~822) coerced the
missing param with ``.unwrap_or_default()``, so ``"" == ""`` passed the
check on a fresh session: the app exchanged the code and logged the
user in (creating the user), and the space ran the whole success chain
before its 500.

The tests stand up a fake self-hosted GitLab (plain HTTP on 127.0.0.1,
an ephemeral port): ``POST /oauth/token`` and ``GET /api/v4/user``. They
seed ``GITLAB_CLIENT_ID`` / ``GITLAB_CLIENT_SECRET`` / ``GITLAB_HOST``
rows (db-sourced on both backends, so no server env change is needed),
drive a cookieless callback with a code and no ``state``, and assert the
redirect plus the absence of side effects (no stub traffic, no ``users``
row). The configured stub is what makes the pre-fix failure real: with
an unconfigured provider the pre-fix app would answer 5111
(``GITLAB_NOT_CONFIGURED``) instead of logging in. Each test mints a
fresh stub email, so the no-user assertions are order-independent.

The config rows are removed afterwards: the rest of this suite asserts
(``oauth_unconfigured``) that no provider is configured, so leaving them
would break sibling modules.

Out of scope (per the issue): ``state=`` present but empty passes on
Django too and is not pinned here.
"""

import json
import os
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402

CONFIG_KEYS = ("GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET", "GITLAB_HOST")


class _FakeGitLab(BaseHTTPRequestHandler):
    server_version = "FakeGitLab1361/1.0"

    def _send_json(self, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        self.server.hits.append(("POST", urlsplit(self.path).path))
        if urlsplit(self.path).path != "/oauth/token":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        self._send_json(
            {
                "access_token": "contract-access-token",
                "token_type": "Bearer",
                "expires_in": 7200,
                "refresh_token": "contract-refresh-token",
                "created_at": int(time.time()),
            }
        )

    def do_GET(self):
        self.server.hits.append(("GET", urlsplit(self.path).path))
        if urlsplit(self.path).path != "/api/v4/user":
            self.send_error(404)
            return
        # No avatar: the flow performs no outbound HTTP beyond the fake.
        self._send_json(
            {
                "id": self.server.provider_id,
                "email": self.server.user_email,
                "name": "No State",
                "username": "nostate1361",
            }
        )

    def log_message(self, *args):
        pass


@pytest.fixture()
def fake_gitlab():
    """Serve the fake provider; yield (base URL, stub email, stub hits)."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), _FakeGitLab)
    server.user_email = f"nostate-{uuid.uuid4().hex[:8]}@example.com"
    server.provider_id = 910000 + (uuid.uuid4().int % 89999)
    server.hits = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", server.user_email, server.hits
    finally:
        server.shutdown()
        thread.join(timeout=10)


@pytest.fixture()
def gitlab_configured(ensure_instance, fake_gitlab):
    """Point both backends' GitLab provider at the fake; clean up after."""
    host, email, hits = fake_gitlab
    database_url = config.database_url()
    values = {
        "GITLAB_CLIENT_ID": "contract-gitlab-id",
        "GITLAB_CLIENT_SECRET": "contract-gitlab-secret",
        "GITLAB_HOST": host,
    }
    for key, value in values.items():
        db.execute(
            database_url,
            "INSERT INTO instance_configurations"
            " (id, key, value, category, is_encrypted, created_at, updated_at)"
            " VALUES (%s, %s, %s, 'general', false, now(), now())"
            " ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value,"
            " is_encrypted = false, deleted_at = NULL, updated_at = now()",
            (str(uuid.uuid4()), key, value),
        )
    try:
        yield host, email, hits
    finally:
        db.execute(
            database_url,
            "DELETE FROM instance_configurations WHERE key = ANY(%s)",
            (list(CONFIG_KEYS),),
        )


def _live_app_origin() -> str:
    """scheme://host the app OAuth redirects are built on.

    Read off a live initiate response (same technique as the conftest
    ``app_base`` fixture, but on a throwaway client: the test client must
    stay cookieless, and initiate would plant a session cookie on it).
    Google stays unconfigured throughout, so this always takes the
    deterministic NOT_CONFIGURED redirect whose origin is the app base.
    """
    with httpx.Client(base_url=config.base_url(), timeout=10, follow_redirects=False) as probe:
        resp = probe.get("/auth/google/")
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    parts = urlsplit(resp.headers["location"])
    return f"{parts.scheme}://{parts.netloc}"


def _assert_no_user(email: str) -> None:
    row = db.fetchone(config.database_url(), "SELECT id, email FROM users WHERE email = %s", (email,))
    assert row is None, f"users row created for {email}: {row!r}"


def test_app_callback_without_state_rejects_with_provider_error(
    client: httpx.Client, gitlab_configured
):
    _, email, hits = gitlab_configured
    origin = _live_app_origin()
    # Fresh client, no session cookie, code but NO state param.
    resp = client.get("/auth/gitlab/callback/", params={"code": "contract-code-no-state"})
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    parts = urlsplit(resp.headers["location"])
    assert f"{parts.scheme}://{parts.netloc}" == origin, resp.headers["location"]
    query = parse_qs(parts.query)
    assert query.get("error_code") == ["5121"], resp.headers["location"]
    assert query.get("error_message") == ["GITLAB_OAUTH_PROVIDER_ERROR"], resp.headers["location"]
    for raw in resp.headers.get_list("set-cookie"):
        assert not raw.startswith(SESSION_COOKIE + "="), f"session cookie set: {raw!r}"
    assert hits == [], f"provider traffic on the mismatch path: {hits!r}"
    _assert_no_user(email)


def test_space_callback_without_state_has_no_side_effects(
    client: httpx.Client, gitlab_configured
):
    _, email, hits = gitlab_configured
    resp = client.get("/auth/spaces/gitlab/callback/", params={"code": "contract-code-no-state"})
    assert resp.status_code == 500, f"expected 500, got {resp.status_code}: {resp.text[:300]}"
    assert hits == [], f"provider traffic on the mismatch path: {hits!r}"
    _assert_no_user(email)
