"""Contract suite: returning-user OAuth login applies User.save() effects (PIDASHCONV-1364).

Django runs the full ``User.save()`` on every OAuth returning login
(``adapter/base.py:347`` → ``save_user_data`` → ``user.save()`` at
``base.py:233``). Per ``pi_dash/db/models/user.py:169-187`` that save
refreshes ``updated_at`` (``auto_now``), backfills a blank
``display_name`` from the email local part, and sets ``is_staff`` for
superusers. The Rust google and gitea ``save_user_data`` UPDATEs wrote
only the login stamps + token + active flag and skipped all three
(github was fixed in PIDASHCONV-1081, the password endpoints in
PIDASHCONV-1352).

Harness choice: gitea accepts a host override, so the success test
points ``GITEA_HOST`` at a loopback stub serving the token + userinfo
endpoints (the PIDASHCONV-1089 pattern) and drives the full app-gitea
callback for a RETURNING user seeded with ``display_name=''``,
``is_superuser``, ``is_staff=false`` and a stale ``updated_at``, then
asserts all three columns on both backends. Google hardcodes its
token/userinfo URLs on both backends (the stage-1 sealed-provider pin,
reaffirmed by PIDASHCONV-1083/1084/1089), so Google has no HTTP success
test here: its net is the ``google_kernel_save_user_data_*`` live test
in ``oauth_google.rs`` plus the UPDATE shape pin below (the
PIDASHCONV-1084 pattern).

The ``GITEA_*`` ``instance_configurations`` rows are inserted
function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module. No
test here contacts a real provider.
"""

import os
import sys
import uuid
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
GOOGLE_RS = REPO_ROOT / "rust-api/crates/api/src/auth_oauth/oauth_google.rs"

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


def _delete_world(store, emails):
    """Best-effort world teardown (accounts/profiles → users); tolerates
    partially seeded worlds."""
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
        conn.commit()


def _gitea_stub_world():
    """Seed a returning user in the pre-fix gap state, behind a stub Gitea.

    Returns (world, cleanup): the stub serves the token exchange plus
    userinfo (email included, so neither backend calls ``/emails``) and
    the emails fallback. Yields the world with the stub running and the
    ``GITEA_*`` config rows in place; cleanup removes the config rows
    first (suite hermeticity), then the world. Context-manager free so a
    failed callback still tears down via finally.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gh1364-{tag}@example.test"
    emails = [email]
    user = store.make_user(email)
    # The gap state: blank display name, superuser without staff, stale
    # updated_at (a week old, so any refresh is unambiguous).
    db.execute(
        database_url,
        "UPDATE users SET display_name = '', is_superuser = TRUE,"
        " is_staff = FALSE, updated_at = now() - make_interval(days => 7)"
        " WHERE id = %s",
        (user["id"],),
    )
    old_updated = db.fetchone(
        database_url, "SELECT updated_at FROM users WHERE id = %s", (user["id"],)
    )["updated_at"]
    world = {"tag": tag, "email": email, "old_updated": old_updated}
    stub_id = int(uuid.uuid4().hex[:8], 16)
    routes = {
        ("POST", "/login/oauth/access_token"): (
            200,
            {"access_token": f"stub-at-{tag}", "token_type": "bearer"},
        ),
        ("GET", "/api/v1/user"): (
            200,
            {
                "id": stub_id,
                "email": email,
                "login": f"gh1364-{tag}",
                "full_name": "Contract User",
                "avatar_url": None,
            },
        ),
        ("GET", "/api/v1/user/emails"): (
            200,
            [{"email": email, "primary": True, "verified": True}],
        ),
    }
    stub = RoutingStub(routes)
    stub.__enter__()
    # Reap before sow: a crashed earlier run must not leak config rows
    # into this one (or, via teardown, into sibling modules).
    _delete_gitea_config(database_url)
    try:
        store.make_config("GITEA_CLIENT_ID", "contract-id", category="AUTHENTICATION")
        store.make_config(
            "GITEA_CLIENT_SECRET", "contract-secret", category="AUTHENTICATION"
        )
        store.make_config("GITEA_HOST", stub.url, category="AUTHENTICATION")
    except Exception:
        _delete_gitea_config(database_url)
        _delete_world(store, emails)
        stub.__exit__(None, None, None)
        raise
    world["stub"] = stub

    def cleanup():
        _delete_gitea_config(database_url)
        _delete_world(store, emails)
        stub.__exit__(None, None, None)

    return world, cleanup


def test_app_gitea_returning_login_applies_save_effects(base_url: str):
    """Full app-gitea login for a returning user applies User.save() effects.

    Runs against both backends. Pre-fix Rust leaves all three columns
    untouched (blank ``display_name``, ``is_staff`` false, stale
    ``updated_at``); post-fix both backends backfill the display name from
    the email local part, flip ``is_staff`` for the superuser, and refresh
    ``updated_at``.
    """
    database_url = config.database_url()
    world, cleanup = _gitea_stub_world()
    email = world["email"]
    state = f"gh1364-state-{world['tag']}"
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

    try:
        assert resp.status_code == 302, (
            f"expected 302, got {resp.status_code}: {resp.text[:300]}"
        )
        location = resp.headers["location"]
        assert location.startswith(SESSION_HOST), location
        assert "error_code" not in parse_qs(urlsplit(location).query), location

        hits = {(r["method"], urlsplit(r["path"]).path) for r in world["stub"].requests}
        assert ("POST", "/login/oauth/access_token") in hits, world["stub"].requests
        assert ("GET", "/api/v1/user") in hits, world["stub"].requests

        row = db.fetchone(
            database_url,
            "SELECT display_name, is_staff, last_login_medium,"
            " updated_at > %s AS refreshed FROM users WHERE email = %s",
            (world["old_updated"], email),
        )
        assert row is not None, "returning user row is gone"
        assert row["last_login_medium"] == "gitea", row
        assert row["display_name"] == email.split("@")[0], row
        assert row["is_staff"] is True, row
        assert row["refreshed"] is True, "updated_at was not refreshed by the login"
    finally:
        cleanup()


def _google_save_update_sql() -> str:
    """The google ``save_user_data`` UPDATE statement, backslashes stripped.

    Harness helper for the shape pin below; asserts its own premises so a
    refactor that moves the statement fails loudly instead of matching
    some other UPDATE.
    """
    source = GOOGLE_RS.read_text()
    fn_at = source.find("pub async fn save_user_data")
    assert fn_at != -1, "harness: save_user_data moved or was renamed"
    update_at = source.find("UPDATE", fn_at)
    assert update_at != -1, "harness: no UPDATE inside save_user_data"
    where_at = source.find("WHERE", update_at)
    assert where_at != -1, "harness: no WHERE inside save_user_data"
    # The statement is the only UPDATE..WHERE span in the function.
    assert source.find("UPDATE", update_at + 1) == -1 or source.find(
        "UPDATE", update_at + 1
    ) > source.find("\n}\n", where_at), "harness: second UPDATE in save_user_data"
    return source[update_at:where_at].replace("\\", "")


def test_google_save_update_applies_save_effects():
    """Regression pin: the google save UPDATE carries the User.save() effects.

    Google logins cannot run over HTTP on either backend (hardcoded
    ``https://`` URLs, no stub seam), so this file pins the statement text
    instead (the PIDASHCONV-1084 pattern): the UPDATE must refresh
    ``updated_at``, backfill a blank ``display_name`` from the email local
    part, and sync ``is_staff`` from ``is_superuser``. FAILS on pre-fix
    code; runs identically in both backend runs. Behavior is pinned by the
    ``google_kernel_save_user_data_*`` live test in ``oauth_google.rs``.
    """
    sql = _google_save_update_sql()
    assert '"updated_at" =' in sql, "save UPDATE must refresh updated_at"
    assert "split_part(" in sql and '"display_name" =' in sql, (
        "save UPDATE must backfill a blank display_name from the email local part"
    )
    assert '"is_staff" = "is_staff" OR "is_superuser"' in sql, (
        "save UPDATE must sync is_staff from is_superuser"
    )
