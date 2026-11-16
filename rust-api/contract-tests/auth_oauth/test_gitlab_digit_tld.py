# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract: GitLab callback accepts digit-TLD provider emails (PIDASHCONV-1360).

Django 4.2.30's ``EmailValidator`` allows digits in the TLD, so a GitLab
user whose address is ``dev@corp.k8s`` signs in normally. The Rust GitLab
port hand-rolled Django 6.0's stricter rule (letters-only TLD) and answers
the same callback with ``error_code=5005`` (``INVALID_EMAIL``).

The test stands up a fake self-hosted GitLab (plain HTTP on 127.0.0.1, an
ephemeral port): ``POST /oauth/token`` and ``GET /api/v4/user``. It seeds
``GITLAB_CLIENT_ID`` / ``GITLAB_CLIENT_SECRET`` / ``GITLAB_HOST`` rows
(db-sourced on both backends, so no server env change is needed), drives
the real initiate -> callback flow, and asserts a clean login redirect
plus the ``users`` row. The fake userinfo carries no avatar, signup is on
and GitLab sync is off by default, so the flow performs no outbound HTTP
beyond the fake.

The config rows are removed afterwards: the rest of this suite asserts
(``oauth_unconfigured``) that no provider is configured, so leaving them
would break sibling modules.
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

EMAIL = "dev@corp.k8s"
PROVIDER_ID = 424242

CONFIG_KEYS = ("GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET", "GITLAB_HOST")


class _FakeGitLab(BaseHTTPRequestHandler):
    server_version = "FakeGitLab/1.0"

    def _send_json(self, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if urlsplit(self.path).path != "/oauth/token":
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        self._send_json(
            {
                "access_token": "fake-access-token",
                "token_type": "Bearer",
                "expires_in": 7200,
                "refresh_token": "fake-refresh-token",
                "created_at": int(time.time()),
            }
        )

    def do_GET(self):
        if urlsplit(self.path).path != "/api/v4/user":
            self.send_error(404)
            return
        self._send_json(
            {
                "id": PROVIDER_ID,
                "email": EMAIL,
                "name": "Dev Corp",
                "username": "devcorp",
            }
        )

    def log_message(self, *args):
        pass


@pytest.fixture()
def fake_gitlab():
    """Serve the fake provider; yield its base URL (``GITLAB_HOST`` value)."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), _FakeGitLab)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        thread.join(timeout=10)


@pytest.fixture()
def gitlab_configured(ensure_instance, fake_gitlab):
    """Point both backends' GitLab provider at the fake; clean up after."""
    database_url = config.database_url()
    values = {
        "GITLAB_CLIENT_ID": "contract-gitlab-id",
        "GITLAB_CLIENT_SECRET": "contract-gitlab-secret",
        "GITLAB_HOST": fake_gitlab,
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
        yield fake_gitlab
    finally:
        db.execute(
            database_url,
            "DELETE FROM instance_configurations WHERE key = ANY(%s)",
            (list(CONFIG_KEYS),),
        )


def test_gitlab_callback_accepts_digit_tld_email(client: httpx.Client, gitlab_configured):
    # Initiate against the fake host: the 302 Location carries the session
    # state (and proves the provider is configured on this backend).
    initiate = client.get("/auth/gitlab/")
    assert initiate.status_code == 302, f"initiate: {initiate.status_code}: {initiate.text[:200]}"
    auth_url = urlsplit(initiate.headers["location"])
    assert (auth_url.scheme, auth_url.netloc) == urlsplit(gitlab_configured)[0:2], initiate.headers["location"]
    state = parse_qs(auth_url.query).get("state", [None])[0]
    assert state, initiate.headers["location"]

    # Complete the callback with the session cookie the initiate set.
    callback = client.get("/auth/gitlab/callback/", params={"code": "contract-code", "state": state})
    assert callback.status_code == 302, f"callback: {callback.status_code}: {callback.text[:300]}"
    location = callback.headers["location"]
    query = parse_qs(urlsplit(location).query)
    assert "error_code" not in query, location
    assert "error_message" not in query, location

    # The login actually landed: the digit-TLD user exists.
    row = db.fetchone(config.database_url(), "SELECT id, email FROM users WHERE email = %s", (EMAIL,))
    assert row is not None, f"no users row for {EMAIL} after clean callback to {location}"
