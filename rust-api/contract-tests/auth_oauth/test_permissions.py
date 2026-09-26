"""Denied-permission case for the OAuth domain (PIDASHCONV-102).

A callback carrying an invalid code MUST NOT create a session: the error
redirect sets no ``session-id`` cookie, and an authenticated probe made
with the callback's cookies still answers 401. Removing the callback's
state guard flips this test red — the request would proceed into provider
construction, which (unconfigured) raises ``GOOGLE_NOT_CONFIGURED`` (5105)
instead of ``GOOGLE_OAUTH_PROVIDER_ERROR`` (5115), so the error-code
assertion below fails. Demonstrated in the PR with a one-line local patch
(``if state != ...`` → ``if False``), reverted.

The positive control logs a seeded user in through the public sign-in flow
and shows the same probe answering 200, proving the probe distinguishes
"no session" from "broken probe".

Known oracle limits (pinned in stage 1):

- The callback success shape (valid code exchanged, session cookie set,
  302 without ``error_code``) cannot run hermetically. Google/GitHub
  hardcode their token/userinfo URLs, so there is no stub seam; GitLab and
  Gitea accept a host override, but pointing it at a stub would mean
  mutating the global ``instance_configurations`` mid-suite, coupling test
  modules by execution order and leaking provider config into sibling
  suites sharing the database. The deterministic hermetic contract is the
  error surface pinned here; the success branch stays covered by the
  provider unit tests in ``apps/api``.
"""

import os
import sys
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402

import uuid  # noqa: E402

from _harness import auth, config, factory  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402

UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}

PROBE = "/api/users/me/"


def test_invalid_code_callback_creates_no_session(client: httpx.Client):
    resp = client.get(
        "/auth/google/callback/",
        params={"code": "contract-test-invalid-code", "state": "no-such-state"},
    )
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    query = parse_qs(urlsplit(resp.headers["location"]).query)
    assert query.get("error_code") == ["5115"]
    for raw in resp.headers.get_list("set-cookie"):
        assert not raw.startswith(SESSION_COOKIE + "="), f"session cookie set: {raw!r}"
    # The callback's cookies must not authenticate anything: the probe that
    # a logged-in user reaches still answers 401 with them.
    cookie_header = "; ".join(
        raw.split(";", 1)[0] for raw in resp.headers.get_list("set-cookie")
    )
    probe = client.get(PROBE, headers={"Cookie": cookie_header} if cookie_header else {})
    assert probe.status_code == 401, (probe.status_code, probe.text[:200])
    assert probe.json() == UNAUTHENTICATED


def test_probe_distinguishes_authenticated_user(client: httpx.Client):
    # Positive control: a user logged in through the public flow reaches the
    # same probe with 200, so the 401 above means "no session", not "dead
    # probe".
    base_url = config.base_url()
    tag = f"oauthdenied{uuid.uuid4().hex[:8]}"
    user = factory.create_user(f"{tag}@example.com")
    try:
        session_cookie = auth.login_session_cookie(base_url, user["email"], user["password"])
        with auth.api_client(base_url, session_cookie) as authed:
            probe = authed.get(PROBE)
        assert probe.status_code == 200, (probe.status_code, probe.text[:200])
    finally:
        factory.cleanup_run(tag)
