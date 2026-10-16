"""Contract suite: app github login side effects (PIDASHCONV-1083).

Django's app github callback emits three side effects past the guards:
``user_activation_email.delay(site, user_id)`` for pre-existing inactive
users (``adapter/base.py:228-231``), and per accepted workspace invite
``invalidate_cache_directly(/api/workspaces/<slug>/members/)`` plus
``track_event.delay`` with ``USER_JOINED_WORKSPACE`` kwargs
(``authentication/utils/workspace_project_join.py:38-56``).

Harness choice (the issue leaves this to the fix run): the HTTP success
path is sealed on BOTH backends — Django posts the exchange to hardcoded
``https://github.com`` / ``https://api.github.com`` URLs
(``provider/oauth/github.py:24-26,111``) and Rust posts to the same
hardcoded consts (``providers::GITHUB_TOKEN_URL`` et al.) over reqwest
with ``rustls-tls-webpki-roots`` (baked Mozilla trust, no env CA), so no
TLS-intercepting provider stub can work without production code changes
or machine-global DNS. Both are forbidden, so there is no provider stub:
the success-path regression net is the kernel-level harness in
``crates/api/src/auth_oauth/oauth_github.rs`` (``github_kernel_*`` live
tests drive ``complete_login_or_signup`` directly with a fabricated
``GithubAuth``; the emits test fails pre-fix and passes post-fix).

What this module pins instead, on both backends over plain HTTP: the
github error branches emit NOTHING. Each test seeds the full
success-world (inactive user, accepted workspace invite, member-list
cache keys that WOULD match the invalidation pattern), drives one
guard branch, and asserts every side-effect channel is unchanged: the
``celery`` broker depth (Django publishes ``.delay()`` there), the
``rust_job_queue`` row count (Rust enqueues there), the user/invite/
membership rows, and the cache keys. The unconfigured-provider branch
(5110, reached only with a matching session ``state``) doubles as proof
the forged sessions below are actually read by the backend.

No test here calls a real provider: the unconfigured-provider branch
returns before any outbound HTTP, like the rest of the suite.
"""

import os
import sys
import uuid
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402

ACTIVATION_TASK = "pi_dash.bgtasks.user_activation_email_task.user_activation_email"
TRACK_EVENT_TASK = "pi_dash.bgtasks.event_tracking_task.track_event"


def _redis_celery_depth():
    """Non-destructive ``celery`` depth on the Django broker lane.

    ``None`` when ``AMQP_URL`` is not a redis URL (the Rust-matrix run
    points it at the AMQP broker, where this path publishes nothing —
    ``auth_oauth`` only ever enqueues into ``rust_job_queue``).
    """
    url = os.environ["AMQP_URL"]
    if not url.startswith("redis"):
        return None
    import redis

    return redis.Redis.from_url(url).llen("celery")


def _queue_table_count(database_url):
    """Row count of ``rust_job_queue``; a missing table counts as empty
    (Django runs never create it — only the Rust worker boot does)."""
    row = db.fetchone(database_url, "SELECT to_regclass('public.rust_job_queue') AS t")
    if not row or not row["t"]:
        return 0
    return db.fetchone(database_url, "SELECT COUNT(*) AS n FROM rust_job_queue")["n"]


def _world_state(database_url, email, workspace_id, user_id):
    user = db.fetchone(
        database_url, "SELECT is_active FROM users WHERE id = %s", (user_id,)
    )
    invites = db.fetchone(
        database_url,
        "SELECT COUNT(*) AS n FROM workspace_member_invites"
        " WHERE email = %s AND accepted AND deleted_at IS NULL",
        (email,),
    )["n"]
    members = db.fetchone(
        database_url,
        "SELECT COUNT(*) AS n FROM workspace_members"
        " WHERE workspace_id = %s AND member_id = %s",
        (workspace_id, user_id),
    )["n"]
    return {"is_active": user["is_active"], "invites_live": invites, "members": members}


def _seed_cache_keys(slug, tag):
    """Member-list keys that WOULD match the per-invite invalidation
    pattern (``*/api/workspaces/<slug>/members/*``), so a stray
    invalidation deletes them and fails the pin."""
    import redis

    client = redis.Redis.from_url(config.REDIS_URL)
    keys = [
        f"gh1083:{tag}:/api/workspaces/{slug}/members/:1",
        f"gh1083:{tag}:/api/workspaces/{slug}/members/:2",
    ]
    for key in keys:
        client.set(key, "1", ex=600)
    return keys


def _cache_keys_present(keys):
    import redis

    client = redis.Redis.from_url(config.REDIS_URL)
    return [key for key in keys if client.exists(key)]


def _delete_cache_keys(keys):
    import redis

    client = redis.Redis.from_url(config.REDIS_URL)
    if keys:
        client.delete(*keys)


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


@pytest.fixture()
def github_world():
    """Inactive user + workspace + accepted invite + matching cache keys.

    Mirrors ``Database.make_*`` (proven seeders); every seeded row is
    deleted afterwards (workspace tables reference the users, so the
    teardown order is members/invites → workspaces → users).
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    owner = store.make_user(f"gh1083-own-{tag}@example.test")
    user = store.make_user(f"gh1083-{tag}@example.test", is_active=False)
    workspace = store.make_workspace(f"gh1083-{tag}", f"gh1083-{tag}", owner["id"])
    store.make_invite(workspace["id"], user["email"], accepted=True)
    keys = _seed_cache_keys(workspace["slug"], tag)
    world = {"user": user, "workspace": workspace, "tag": tag, "cache_keys": keys}
    yield world
    _delete_cache_keys(keys)
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM workspace_members WHERE member_id = %s", (user["id"],)
            )
            cur.execute(
                "DELETE FROM workspace_member_invites WHERE email = %s",
                (user["email"],),
            )
            cur.execute("DELETE FROM workspaces WHERE id = %s", (workspace["id"],))
            cur.execute("DELETE FROM users WHERE id IN (%s, %s)", (user["id"], owner["id"]))
        conn.commit()


def _snapshot(database_url, world):
    return {
        "celery": _redis_celery_depth(),
        "queue": _queue_table_count(database_url),
        "world": _world_state(
            database_url,
            world["user"]["email"],
            world["workspace"]["id"],
            world["user"]["id"],
        ),
    }


def _assert_unchanged(before, database_url, world):
    after = _snapshot(database_url, world)
    assert after == before, f"side effect emitted: {before} -> {after}"
    assert _cache_keys_present(world["cache_keys"]) == world["cache_keys"]


def _assert_error_redirect(resp, error_code):
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    query = parse_qs(urlsplit(resp.headers["location"]).query)
    assert query.get("error_code") == [error_code], resp.headers["location"]


@pytest.mark.parametrize("branch", ["state-mismatch", "missing-code", "unconfigured"])
def test_app_github_error_branches_emit_no_side_effects(
    client: httpx.Client, base_url: str, github_world, branch: str
):
    """Guard branches answer their redirect and touch nothing else.

    The seeded world is the full success-world (inactive user, accepted
    invite, matching cache keys); every channel must read identical
    before and after. Runs against both backends.
    """
    database_url = config.database_url()
    before = _snapshot(database_url, github_world)
    state = f"gh1083-state-{github_world['tag']}"

    if branch == "state-mismatch":
        # Fresh client, no session: the state check fires first.
        resp = client.get(
            "/auth/github/callback/",
            params={"code": "contract-test-invalid-code", "state": "no-such-state"},
        )
        _assert_error_redirect(resp, "5120")
    else:
        session_key = _forge_oauth_session(database_url, state)
        try:
            authed = httpx.Client(
                base_url=base_url,
                cookies={SESSION_COOKIE: session_key},
                timeout=10,
                follow_redirects=False,
            )
            try:
                if branch == "missing-code":
                    resp = authed.get("/auth/github/callback/", params={"state": state})
                    _assert_error_redirect(resp, "5120")
                else:
                    # Matching state + code, but no provider credentials:
                    # NOT_CONFIGURED (5110). A forged-state failure would
                    # land on the mismatch branch (5120) instead, so 5110
                    # proves the session above was actually read.
                    resp = authed.get(
                        "/auth/github/callback/",
                        params={"code": "contract-test-invalid-code", "state": state},
                    )
                    _assert_error_redirect(resp, "5110")
            finally:
                authed.close()
        finally:
            db.execute(
                database_url,
                "DELETE FROM sessions WHERE session_key = %s",
                (session_key,),
            )

    _assert_unchanged(before, database_url, github_world)
