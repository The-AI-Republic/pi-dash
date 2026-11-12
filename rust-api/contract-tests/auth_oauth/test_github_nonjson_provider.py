"""Contract suite: github callback maps a non-JSON provider body to 5120 (NEWPIDASH-18).

Django decodes the token, userinfo and emails bodies inside
``try ... except requests.RequestException``
(``adapter/oauth.py:76-84`` and ``:86-100``;
``provider/oauth/github.py:109-135`` for emails), and
``requests.exceptions.JSONDecodeError`` has subclassed
``RequestException`` since requests 2.27, so a 200 with a non-JSON body
becomes ``AuthenticationException(5120 GITHUB_OAUTH_PROVIDER_ERROR)``
and a 302 to the sign-in page. The Rust port mapped the decode failure
to a bare 500 on all three legs.

How the stub reaches each backend (PIDASHCONV-1472 precedent): neither
backend takes a provider-base override in production (both hardcode
``https://github.com`` / ``https://api.github.com``), so each leg uses
its own mechanism over one shared route table (:func:`token_routes`):

- Rust leg (``GITHUB_OAUTH_STUB_MODE=plain``): the server boots with
  ``PIDASH_GITHUB_OAUTH_BASE`` pointing at a plain-HTTP
  :class:`_harness.sinks.RedirectStub` on fixed ``GITHUB_OAUTH_STUB_PORT``.
  That variable is the only product-code addition, unset in production.
- Django leg (``GITHUB_OAUTH_STUB_MODE=mitm``): the server boots with
  ``HTTPS_PROXY`` pointing at :class:`_harness.sinks.MitmProxy` (fixed
  ``GITHUB_OAUTH_MITM_PORT``, MITMing ``github.com`` only — the token
  leg fails before userinfo/emails are ever called) and
  ``REQUESTS_CA_BUNDLE`` at a bundle this module writes (test CA minted
  per run + system roots) to ``GITHUB_OAUTH_CA_BUNDLE``. No Python file
  changed.

Without ``GITHUB_OAUTH_STUB_MODE`` every test here skips before touching
the network, so the standard contract jobs (servers booted without stub
env) never contact github.com. With the mode set but the server not
stubbed, the loop-membership assertion fails loudly instead of hitting
the real network and passing by accident.

Unlike the rest of this directory (see ``conftest.py`` hermeticity), this
module configures the provider: it upserts plaintext
``instance_configurations`` rows for ``GITHUB_CLIENT_ID`` /
``GITHUB_CLIENT_SECRET`` (both db-source keys) and deletes/restores them
in ``finally``, so the session-scoped ``oauth_unconfigured`` gate other
modules use never observes them.
"""

import os
import sys
import uuid
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db, sinks  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402

STUB_PORT = int(os.environ.get("GITHUB_OAUTH_STUB_PORT", "14831"))
MITM_PORT = int(os.environ.get("GITHUB_OAUTH_MITM_PORT", "14832"))
CA_BUNDLE_PATH = os.environ.get(
    "GITHUB_OAUTH_CA_BUNDLE", "/tmp/pidash-np18-ca-bundle.pem"
)
STUB_MODE = os.environ.get("GITHUB_OAUTH_STUB_MODE", "")

CONFIG_KEYS = ["GITHUB_CLIENT_ID", "GITHUB_CLIENT_SECRET"]

# An intermediary error page: 200, text/html, undecodable as JSON.
NON_JSON_BODY = "<html><body>Bad Gateway</body></html>"


def token_routes():
    """Shared route table: the token exchange answers 200 non-JSON."""
    return {
        ("POST", "/login/oauth/access_token"): (
            200,
            NON_JSON_BODY,
            {"Content-Type": "text/html"},
        ),
    }


def _save_instance_config(database_url, keys):
    rows = {
        row["key"]: row
        for row in db.fetchall(
            database_url,
            "SELECT key, value, category, is_encrypted FROM instance_configurations"
            " WHERE key = ANY(%s) AND deleted_at IS NULL",
            (keys,),
        )
    }
    return {key: rows.get(key) for key in keys}


def _set_instance_config(database_url, values):
    for key, value in values.items():
        db.execute(
            database_url,
            "INSERT INTO instance_configurations"
            " (id, key, value, category, is_encrypted, created_at, updated_at)"
            " VALUES (%s, %s, %s, 'GITHUB', false, now(), now())"
            " ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value,"
            " deleted_at = NULL, updated_at = now()",
            (db.new_uuid(), key, value),
        )


def _restore_instance_config(database_url, saved):
    for key, row in saved.items():
        if row is None:
            db.execute(
                database_url,
                "DELETE FROM instance_configurations WHERE key = %s",
                (key,),
            )
        else:
            db.execute(
                database_url,
                "UPDATE instance_configurations SET value = %s, category = %s,"
                " is_encrypted = %s, updated_at = now() WHERE key = %s",
                (row["value"], row["category"], row["is_encrypted"], key),
            )


def _forge_oauth_session(database_url, state):
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


def _row_counts(database_url):
    users = db.fetchone(database_url, "SELECT COUNT(*) AS n FROM users")["n"]
    accounts = db.fetchone(database_url, "SELECT COUNT(*) AS n FROM accounts")["n"]
    return {"users": users, "accounts": accounts}


def _drive_callback(base_url, session_key, state):
    authed = httpx.Client(
        base_url=base_url,
        cookies={SESSION_COOKIE: session_key},
        timeout=30,
        follow_redirects=False,
    )
    try:
        return authed.get(
            "/auth/github/callback/",
            params={"code": "contract-test-code", "state": state},
        )
    finally:
        authed.close()


def _assert_provider_error_redirect(resp):
    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    )
    query = parse_qs(urlsplit(resp.headers["location"]).query)
    assert query.get("error_code") == ["5120"], resp.headers["location"]
    assert query.get("error_message") == ["GITHUB_OAUTH_PROVIDER_ERROR"], (
        resp.headers["location"]
    )


def test_github_nonjson_token_body_redirects_provider_error(
    base_url, ensure_instance, tmp_path_factory
):
    """A 200 non-JSON token body is a 5120 redirect that writes nothing.

    Drives the app callback with a matching session state and configured
    (stubbed) provider credentials. Pre-fix Rust answers 500 here; Django
    and fixed Rust answer 302 with the provider-error banner.
    """
    if STUB_MODE not in ("plain", "mitm"):
        pytest.skip(
            "OAuth provider stub not wired: set GITHUB_OAUTH_STUB_MODE=plain"
            " (Rust booted with PIDASH_GITHUB_OAUTH_BASE) or =mitm (Django"
            " booted with HTTPS_PROXY + REQUESTS_CA_BUNDLE)."
        )
    database_url = config.database_url()
    saved = _save_instance_config(database_url, CONFIG_KEYS)
    _set_instance_config(
        database_url,
        {
            "GITHUB_CLIENT_ID": "contract-oauth-id",
            "GITHUB_CLIENT_SECRET": "contract-oauth-secret",
        },
    )
    state = f"gh18-state-{uuid.uuid4().hex[:8]}"
    session_key = _forge_oauth_session(database_url, state)
    before = _row_counts(database_url)
    try:
        if STUB_MODE == "plain":
            with sinks.RedirectStub(token_routes(), port=STUB_PORT) as stub:
                resp = _drive_callback(base_url, session_key, state)
                hits = [
                    entry
                    for entry in stub.snapshot()
                    if entry["method"] == "POST"
                    and entry["path"].startswith("/login/oauth/access_token")
                ]
                assert hits, (
                    "stub saw no token POST: server not booted with"
                    " PIDASH_GITHUB_OAUTH_BASE?"
                )
        else:
            bundle, cert, key = sinks.make_ca_bundle(
                tmp_path_factory.mktemp("np18-mitm-ca"), sans=("github.com",)
            )
            os.replace(bundle, CA_BUNDLE_PATH)
            with sinks.MitmProxy(
                token_routes(),
                mitm_host="github.com",
                tls_cert=cert,
                tls_key=key,
                port=MITM_PORT,
            ) as proxy:
                resp = _drive_callback(base_url, session_key, state)
                assert proxy.saw_connect("github.com"), (
                    "proxy saw no CONNECT github.com: server not booted"
                    " with HTTPS_PROXY?"
                )
        _assert_provider_error_redirect(resp)
    finally:
        db.execute(
            database_url,
            "DELETE FROM sessions WHERE session_key = %s",
            (session_key,),
        )
        _restore_instance_config(database_url, saved)
    after = _row_counts(database_url)
    assert after == before, f"rows written on the provider-error path: {before} -> {after}"
