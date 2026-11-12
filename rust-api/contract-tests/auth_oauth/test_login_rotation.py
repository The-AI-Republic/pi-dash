"""Contract suite: OAuth login session semantics (PIDASHCONV-1357).

Django's ``user_login`` (``authentication/utils/login.py:15``) calls
``django.contrib.auth.login()``, which rotates the session key (``flush()``
on a different user or stale auth hash, ``cycle_key()`` when anonymous),
stamps ``users.last_login`` via the ``user_logged_in`` signal, and rotates
the CSRF token. The Rust github/gitlab/gitea callbacks used to write the
``_auth_*`` trio onto the live session and stop there: the pre-login key
stayed valid (session fixation), ``last_login`` stayed NULL, and no
``csrftoken`` was issued.

Harness choice: gitlab and gitea accept host overrides, so the success
tests point ``GITLAB_HOST`` / ``GITEA_HOST`` at a loopback stub serving
the token + userinfo endpoints and drive the full app-callback success
path over plain HTTP on both backends (the PIDASHCONV-1089 pattern).
Github hardcodes its token/userinfo URLs on both backends (the sealed
provider of PIDASHCONV-1083), so it has no HTTP success test here: its
net is the kernel-level harness (``login_kernel_*`` live tests in
``crates/api/src/auth_oauth/login.rs`` driving the shared helper
directly) plus the shared-helper coverage the gitlab/gitea tests below
provide — all four providers call the same ``login_session_keys``.

The ``GITLAB_*`` / ``GITEA_*`` ``instance_configurations`` rows are
inserted per-test and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests
run serially; no fixture can observe the rows mid-test). No test here
contacts a real provider.
"""

import base64
import contextlib
import hashlib
import hmac
import json
import os
import sys
import uuid
import zlib
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import (  # noqa: E402
    AUTH_BACKEND,
    SESSION_KEY_SALT,
    forge_session_data,
    session_auth_hash,
)
from _harness.sinks import RoutingStub  # noqa: E402

CONFIG_KEYS = (
    "GITLAB_CLIENT_ID",
    "GITLAB_CLIENT_SECRET",
    "GITLAB_HOST",
    "GITEA_CLIENT_ID",
    "GITEA_CLIENT_SECRET",
    "GITEA_HOST",
)

PROVIDERS = ("gitlab", "gitea")

#: ``request.META["CSRF_COOKIE"]`` Django 4.2 sets on ``rotate_token``: the
#: raw 32-char secret (``CSRF_COOKIE_MASKED`` defaults False, and this
#: project never sets it).
CSRF_VALUE_LENGTH = 32


def _decode_session(stored: str, secret: str) -> dict:
    """Decode a ``sessions.session_data`` value (Django ``TimestampSigner``
    format: ``base64d:timestamp:signature``)."""
    base64d, ts, sig = stored.split(":")
    value = f"{base64d}:{ts}"
    key = hashlib.sha256((SESSION_KEY_SALT + "signer" + secret).encode()).digest()
    expect = (
        base64.urlsafe_b64encode(
            hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256).digest()
        )
        .rstrip(b"=")
        .decode()
    )
    assert hmac.compare_digest(expect, sig), "session signature mismatch"
    if base64d.startswith("."):
        raw = zlib.decompress(
            base64.urlsafe_b64decode(base64d[1:] + "=" * (-len(base64d[1:]) % 4))
        )
    else:
        raw = base64.urlsafe_b64decode(base64d + "=" * (-len(base64d) % 4))
    return json.loads(raw.decode("latin-1"))


def _session_row(database_url, key):
    return db.fetchone(
        database_url, "SELECT session_data FROM sessions WHERE session_key = %s", (key,)
    )


def _cookies_of(resp, name):
    """(value, raw Set-Cookie) pairs for one cookie name."""
    out = []
    for raw in resp.headers.get_list("set-cookie"):
        head = raw.split(";", 1)[0]
        if head.startswith(name + "="):
            out.append((head[len(name) + 1 :], raw))
    return out


def _delete_config(database_url):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(CONFIG_KEYS),),
    )


def _delete_world(store, emails, session_keys):
    """Best-effort teardown (accounts/profiles/prefs → users, sessions);
    tolerates partially built worlds."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM accounts WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM profiles WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id IN"
                " (SELECT id FROM users WHERE email = ANY(%s))",
                (emails,),
            )
            cur.execute("DELETE FROM users WHERE email = ANY(%s)", (emails,))
            for key in session_keys:
                cur.execute("DELETE FROM sessions WHERE session_key = %s", (key,))
        conn.commit()


def _routes(provider, email, tag):
    if provider == "gitlab":
        return {
            ("POST", "/oauth/token"): (
                200,
                {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
            ),
            ("GET", "/api/v4/user"): (
                200,
                {
                    "id": 771771,
                    "email": email,
                    "name": "Contract User",
                    "avatar_url": None,
                },
            ),
        }
    return {
        ("POST", "/login/oauth/access_token"): (
            200,
            {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
        ),
        ("GET", "/api/v1/user"): (
            200,
            {
                "id": 424242,
                "email": email,
                "login": f"pb1357-{tag}",
                "full_name": "Contract User",
                "avatar_url": None,
            },
        ),
        ("GET", "/api/v1/user/emails"): (
            200,
            [{"email": email, "primary": True, "verified": True}],
        ),
    }


@contextlib.contextmanager
def _stubbed_provider(store, database_url, provider, email, tag):
    """Run one provider's stub and install its config rows; teardown
    removes the rows first (suite hermeticity)."""
    prefix = provider.upper()
    with RoutingStub(_routes(provider, email, tag)) as stub:
        # Reap before sow: a crashed earlier run must not leak config rows
        # into this one (or, via teardown, into sibling modules).
        _delete_config(database_url)
        try:
            store.make_config(f"{prefix}_CLIENT_ID", "cid", category="AUTHENTICATION")
            store.make_config(
                f"{prefix}_CLIENT_SECRET", "csec", category="AUTHENTICATION"
            )
            store.make_config(f"{prefix}_HOST", stub.url, category="AUTHENTICATION")
        except Exception:
            _delete_config(database_url)
            raise
        try:
            yield stub
        finally:
            _delete_config(database_url)


def _assert_csrf_rotated(resp, planted):
    """The callback answers a fresh ``csrftoken`` cookie: Django 4.2's raw
    32-char secret with the stock attributes (no ``Domain``/``Secure`` on
    plain HTTP)."""
    found = _cookies_of(resp, "csrftoken")
    assert len(found) == 1, f"one csrftoken cookie: {resp.headers.get_list('set-cookie')!r}"
    value, raw = found[0]
    assert value != planted, "rotated, not echoed"
    assert len(value) == CSRF_VALUE_LENGTH, value
    assert value.isalnum(), value
    assert "; HttpOnly" in raw, raw
    assert "; Max-Age=31449600" in raw, raw
    assert "; Path=/" in raw, raw
    assert "; SameSite=Lax" in raw, raw
    assert "Domain=" not in raw, raw
    assert "; Secure" not in raw, raw


@pytest.mark.parametrize("provider", PROVIDERS)
# ensure_instance: initiate 302s INSTANCE_NOT_CONFIGURED (no `state`) on a
# fresh database without the setup marker, so this file must not rely on
# another module's session fixture having seeded it.
def test_anonymous_login_rotates_session_and_stamps_last_login(
    base_url: str, provider: str, ensure_instance
):
    """Full app login from an anonymous initiate session: the callback 302s
    without ``error_code``, the session key rotates (old row gone, new row
    bound to the created user), ``last_login`` is stamped, anonymous data
    (``state``/``host``) survives the cycle, and a fresh ``csrftoken`` is
    issued. Runs against both backends; fails on pre-fix Rust (same key
    re-issued, NULL ``last_login``, no ``csrftoken``)."""
    database_url = config.database_url()
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"pb1357-{provider}-{tag}@example.test"
    session_keys: list[str] = []
    try:
        with _stubbed_provider(store, database_url, provider, email, tag):
            client = httpx.Client(
                base_url=base_url, timeout=30, follow_redirects=False
            )
            try:
                initiate = client.get(f"/auth/{provider}/")
                assert initiate.status_code == 302, initiate.text[:200]
                old = _cookies_of(initiate, SESSION_COOKIE)
                assert len(old) == 1, initiate.headers.get_list("set-cookie")
                old_key = old[0][0]
                session_keys.append(old_key)
                pre = _session_row(database_url, old_key)
                assert pre is not None, "initiate persists its session"
                state = _decode_session(pre["session_data"], secret).get("state")
                assert state, "initiate stores state"
                planted_csrf = "F" * 64
                client.cookies.set(SESSION_COOKIE, old_key)
                client.cookies.set("csrftoken", planted_csrf)
                resp = client.get(
                    f"/auth/{provider}/callback/",
                    params={"code": "contract-stub-code", "state": state},
                )
            finally:
                client.close()
        assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
        query = parse_qs(urlsplit(resp.headers["location"]).query)
        assert "error_code" not in query, resp.headers["location"]

        new = _cookies_of(resp, SESSION_COOKIE)
        assert len(new) == 1, resp.headers.get_list("set-cookie")
        new_key = new[0][0]
        session_keys.append(new_key)
        assert new_key != old_key, "session key rotates on login"

        assert _session_row(database_url, old_key) is None, "old key no longer resolves"
        row = _session_row(database_url, new_key)
        assert row is not None, "new session persists"
        data = _decode_session(row["session_data"], secret)
        user = db.fetchone(
            database_url, "SELECT id, last_login FROM users WHERE email = %s", (email,)
        )
        assert user is not None, "callback created the user"
        assert data.get("_auth_user_id") == str(user["id"]), data
        assert data.get("_auth_user_backend") == AUTH_BACKEND, data
        assert data.get("device_info", {}).get("domain"), data
        # Anonymous `cycle_key` keeps the data: the pre-login keys survive.
        assert data.get("state") == state, data
        assert data.get("host"), data
        assert user["last_login"] is not None, "last_login stamped"
        _assert_csrf_rotated(resp, planted_csrf)
    finally:
        _delete_world(store, [email], session_keys)


@pytest.mark.parametrize("provider", PROVIDERS)
def test_login_on_other_user_session_flushes(
    base_url: str, provider: str, ensure_instance
):
    """A login on another user's session flushes it: the callback 302s, the
    new key differs, A's row is gone, the new session carries only B's
    login keys (A's marker dropped), and B's ``last_login`` is stamped.
    Runs against both backends; fails on pre-fix Rust (A's key reused,
    marker survives, NULL ``last_login``)."""
    database_url = config.database_url()
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email_a = f"pb1357-{provider}-a-{tag}@example.test"
    email_b = f"pb1357-{provider}-b-{tag}@example.test"
    session_keys: list[str] = []
    user_a = store.make_user(email_a)
    try:
        state = f"pb1357-state-{tag}"
        payload = {
            "_auth_user_id": str(user_a["id"]),
            "_auth_user_backend": AUTH_BACKEND,
            "_auth_user_hash": session_auth_hash(user_a["password_hash"], secret),
            "probe_a_marker": "A123",
            "host": "http://oauth-state.test",
            "state": state,
        }
        key_a = uuid.uuid4().hex
        db.execute(
            database_url,
            "INSERT INTO sessions (session_key, session_data, expire_date)"
            " VALUES (%s, %s, now() + make_interval(days => 7))",
            (key_a, forge_session_data(payload, secret)),
        )
        session_keys.append(key_a)
        with _stubbed_provider(store, database_url, provider, email_b, tag):
            client = httpx.Client(
                base_url=base_url,
                cookies={SESSION_COOKIE: key_a},
                timeout=30,
                follow_redirects=False,
            )
            try:
                resp = client.get(
                    f"/auth/{provider}/callback/",
                    params={"code": "contract-stub-code", "state": state},
                )
            finally:
                client.close()
        assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:300]}"
        query = parse_qs(urlsplit(resp.headers["location"]).query)
        assert "error_code" not in query, resp.headers["location"]

        new = _cookies_of(resp, SESSION_COOKIE)
        assert len(new) == 1, resp.headers.get_list("set-cookie")
        key_b = new[0][0]
        session_keys.append(key_b)
        assert key_b != key_a, "B's login mints a fresh key"

        assert _session_row(database_url, key_a) is None, "A's key no longer resolves"
        row = _session_row(database_url, key_b)
        assert row is not None, "B's session persists"
        data = _decode_session(row["session_data"], secret)
        user_b = db.fetchone(
            database_url, "SELECT id, last_login FROM users WHERE email = %s", (email_b,)
        )
        assert user_b is not None, "callback created user B"
        assert data.get("_auth_user_id") == str(user_b["id"]), data
        # `flush()` wipes the data: exactly the four login keys survive.
        assert sorted(data) == [
            "_auth_user_backend",
            "_auth_user_hash",
            "_auth_user_id",
            "device_info",
        ], sorted(data)
        assert user_b["last_login"] is not None, "B's last_login stamped"
        _assert_csrf_rotated(resp, "F" * 64)
    finally:
        _delete_world(store, [email_a, email_b], session_keys)
