"""Contract suite: OAuth signup creates the notification-preferences row (PIDASHCONV-1528).

Django's ``User`` post_save receiver ``create_user_notification``
(``db/models/user.py:307-320``) inserts a ``UserNotificationPreference``
row — all five flags true, NULL audit/scope columns — for every new
non-bot user, whatever the signup path. Rust's GitHub, Google and Gitea
OAuth signups skipped it (email, magic, admin and GitLab signups insert
it by hand), so those users get notification behavior that differs from
Django wherever preferences are read (``jobs/src/tasks_mail/``).

Harness choice: Gitea accepts a host override, so — like PIDASHCONV-1362 —
this module points ``GITEA_HOST`` at a loopback stub serving the token +
userinfo endpoints and drives a full app-gitea callback signup (fresh
email, no pre-seeded user) over plain HTTP on both backends, then asserts
exactly one ``user_notification_preferences`` row with Django's defaults.
Pre-fix Rust creates the user with no prefs row and fails; Django and
post-fix Rust pass. GitHub and Google have hardcoded https provider URLs
with no stub seam on either backend (see ``test_github_side_effects.py``
and ``test_save_semantics_oracle.py``), so their signup paths are pinned
by the Rust kernel tests in ``oauth_github.rs`` / ``oauth_google.rs``
instead. No test here contacts a real provider.

Scope notes: the ``GITEA_*`` ``instance_configurations`` rows are inserted
function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests run
serially; no fixture can observe the rows mid-test). The userinfo payload
carries the email (neither backend calls ``/emails``) and a null avatar
(no S3 traffic on either backend).
"""

import os
import sys
import uuid
from urllib.parse import urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402

GITEA_KEYS = ("GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET", "GITEA_HOST")
SESSION_HOST = "http://oauth-state.test"


def _forge_oauth_session(database_url, state):
    """Forge a ``sessions`` row carrying an OAuth ``state`` + ``host`` (the
    fixed ``CONTRACT_SECRET_KEY`` signs it exactly like Django). The
    success redirect is ``urljoin(host, path)``, so ``host`` must be set or
    the callback 500s after authenticating. Returns the session key; the
    caller deletes the row."""
    secret = os.environ[config.CONTRACT_SECRET_KEY]
    payload = {"host": SESSION_HOST, "state": state}
    session_key = uuid.uuid4().hex
    db.execute(
        database_url,
        "INSERT INTO sessions (session_key, session_data, expire_date)"
        " VALUES (%s, %s, now() + make_interval(days => 7))",
        (session_key, forge_session_data(payload, secret)),
    )
    return session_key


def _delete_session(database_url, session_key):
    db.execute(
        database_url, "DELETE FROM sessions WHERE session_key = %s", (session_key,)
    )


def _delete_gitea_config(database_url):
    db.execute(
        database_url,
        "DELETE FROM instance_configurations WHERE key = ANY(%s)",
        (list(GITEA_KEYS),),
    )


def _delete_world(store, user_id):
    """Best-effort user teardown (prefs/accounts/profiles → users). The
    prefs row must go before the users row: its ``user_id`` FK is
    RESTRICT, not CASCADE."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM user_notification_preferences WHERE user_id = %s",
                (user_id,),
            )
            cur.execute("DELETE FROM accounts WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM profiles WHERE user_id = %s", (user_id,))
            cur.execute(
                "DELETE FROM workspace_members WHERE member_id = %s", (user_id,)
            )
            cur.execute("DELETE FROM users WHERE id = %s", (user_id,))
        conn.commit()


@pytest.fixture()
def gitea_signup_world():
    """Fresh email behind a stub Gitea (no pre-seeded user, so the
    callback takes the signup branch).

    The stub serves the token exchange plus userinfo (email included, so
    neither backend calls ``/emails``; null avatar, so neither backend
    touches S3) and the emails fallback. Yields the world with the stub
    running and the ``GITEA_*`` config rows in place; teardown removes
    the config rows first (suite hermeticity), then the user.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"np1528-{tag}@example.test"
    world = {"tag": tag, "email": email}
    routes = {
        ("POST", "/login/oauth/access_token"): (
            200,
            {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
        ),
        ("GET", "/api/v1/user"): (
            200,
            {
                "id": 424242,
                "email": email,
                "login": f"np1528-{tag}",
                "full_name": "Prefs User",
                "avatar_url": None,
            },
        ),
        ("GET", "/api/v1/user/emails"): (
            200,
            [{"email": email, "primary": True, "verified": True}],
        ),
    }
    with RoutingStub(routes) as stub:
        # Reap before sow: a crashed earlier run must not leak config rows
        # into this one (or, via teardown, into sibling modules).
        _delete_gitea_config(database_url)
        try:
            store.make_config(
                "GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION"
            )
            store.make_config(
                "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
            )
            store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
        except Exception:
            _delete_gitea_config(database_url)
            raise
        try:
            world["stub"] = stub
            yield world
        finally:
            _delete_gitea_config(database_url)
            row = db.fetchone(
                database_url, "SELECT id FROM users WHERE email = %s", (email,)
            )
            if row is not None:
                _delete_world(store, row["id"])


def test_app_gitea_signup_creates_notification_prefs(base_url: str, gitea_signup_world):
    """A fresh gitea signup leaves one prefs row with Django's defaults.

    ``create_user_notification`` (``user.py:307-320``) fires on every
    non-bot create: one ``user_notification_preferences`` row, all five
    flags true, NULL audit/scope columns, live (``deleted_at`` NULL).
    Pre-fix Rust creates the user with no prefs row.
    """
    database_url = config.database_url()
    world = gitea_signup_world
    email = world["email"]

    state = f"np1528-state-{world['tag']}"
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
                params={"code": "contract-stub-code", "state": state},
            )
        finally:
            authed.close()
    finally:
        _delete_session(database_url, session_key)

    assert resp.status_code == 302, (
        f"expected 302, got {resp.status_code}: {resp.text[:300]}"
    )
    location = resp.headers["location"]
    assert location.startswith(SESSION_HOST), location
    assert "error_code" not in location, location

    hits = {(r["method"], urlsplit(r["path"]).path) for r in world["stub"].requests}
    assert ("POST", "/login/oauth/access_token") in hits, world["stub"].requests
    assert ("GET", "/api/v1/user") in hits, world["stub"].requests

    user = db.fetchone(
        database_url, "SELECT id, is_active FROM users WHERE email = %s", (email,)
    )
    assert user is not None, "signup created no users row"
    assert user["is_active"], "new user is active"

    prefs = db.fetchall(
        database_url,
        "SELECT property_change, state_change, comment, mention,"
        " issue_completed, deleted_at, workspace_id, project_id,"
        " created_by_id, updated_by_id"
        " FROM user_notification_preferences WHERE user_id = %s",
        (user["id"],),
    )
    assert len(prefs) == 1, f"expected one prefs row, got {len(prefs)}"
    row = prefs[0]
    assert row["property_change"] is True, row
    assert row["state_change"] is True, row
    assert row["comment"] is True, row
    assert row["mention"] is True, row
    assert row["issue_completed"] is True, row
    assert row["deleted_at"] is None, row
    assert row["workspace_id"] is None, row
    assert row["project_id"] is None, row
    assert row["created_by_id"] is None, row
    assert row["updated_by_id"] is None, row
