"""Contract suite: gitea activation site is WEB_URL, not the request Host (PIDASHCONV-1362).

When a pre-existing inactive user logs in, Django queues
``user_activation_email.delay(base_host(request=self.request), user.id)``
(``authentication/adapter/base.py:228-231``), and ``base_host`` resolves to
``settings.WEB_URL or settings.APP_BASE_URL`` (``utils/host.py:25-28``) —
never the request host. The task builds
``profile_url = current_site + "/profile"``
(``bgtasks/user_activation_email_task.py:29``).

Rust's gitea callback instead built ``site`` from the request Host header
plus the forwarded scheme (``oauth_gitea.rs`` inactive tail), so behind
the edge proxy the mailed link points at an internal API host. Github,
google and gitlab already use ``shapes::base_host``.

Harness choice: gitea accepts a host override, so — like PIDASHCONV-1089 —
the test points ``GITEA_HOST`` at a loopback stub serving the token +
userinfo endpoints and drives the full app-gitea callback success path
over plain HTTP on both backends, with a ``Host`` header (``api.internal``)
that differs from ``WEB_URL``. The queued activation task is observed on
each backend's own channel — Django ``.delay()`` publishes to the redis
``celery`` list (no eager flag in ``settings/test.py``; no worker runs in
HTTP suites so the message persists), Rust enqueues a ``rust_job_queue``
row — and exactly one site is asserted equal to ``CONTRACT_WEB_URL``
(which must match the server's ``WEB_URL``). Pre-fix Rust yields
``http://api.internal:8000`` and fails; Django and post-fix Rust pass.

Scope notes: the ``GITEA_*`` ``instance_configurations`` rows are inserted
function-scoped and deleted in teardown, so the suite's boot-time
``oauth_unconfigured`` hermeticity holds for every other module (tests run
serially; no fixture can observe the rows mid-test). No workspace or
invite is seeded: the probe showed the inactive-existing-user path 302s
to onboarding with exactly one queued task on both backends. No test here
contacts a real provider.
"""

import base64
import json
import os
import sys
import uuid
from collections import Counter
from urllib.parse import urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db, env  # noqa: E402
from _harness.client import SESSION_COOKIE  # noqa: E402
from _harness.db import Database  # noqa: E402
from _harness.http import forge_session_data  # noqa: E402
from _harness.sinks import RoutingStub  # noqa: E402

ACTIVATION_TASK = "pi_dash.bgtasks.user_activation_email_task.user_activation_email"
GITEA_KEYS = ("GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET", "GITEA_HOST")
SESSION_HOST = "http://oauth-state.test"
# Must differ from the WEB_URL host the backend booted with (asserted in
# the test): pre-fix Rust echoes this authority back as the site.
REQUEST_HOST = "api.internal:8000"


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
    """Best-effort user teardown (accounts/profiles → users)."""
    with store.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM accounts WHERE user_id = %s", (user_id,))
            cur.execute("DELETE FROM profiles WHERE user_id = %s", (user_id,))
            cur.execute(
                "DELETE FROM workspace_members WHERE member_id = %s", (user_id,)
            )
            cur.execute("DELETE FROM users WHERE id = %s", (user_id,))
        conn.commit()


def _redis_client_or_none():
    """Redis client for the Django broker lane, or None when the broker is
    not redis (mirrors the PIDASHCONV-1083 depth guard)."""
    url = os.environ.get("AMQP_URL", "")
    if not url.startswith("redis"):
        return None
    import redis

    return redis.Redis.from_url(url)


def _celery_task_args(raw):
    """Parse one redis ``celery`` list entry into (task_name, args)."""
    msg = json.loads(raw.decode())
    body = msg.get("body")
    if isinstance(body, str):
        try:
            body = json.loads(body)
        except ValueError:
            body = json.loads(base64.b64decode(body).decode())
    args = list(body[0]) if isinstance(body, (list, tuple)) else []
    return (msg.get("headers") or {}).get("task"), args


def _queue_table_present(database_url):
    row = db.fetchone(database_url, "SELECT to_regclass('public.rust_job_queue') AS t")
    return bool(row and row["t"])


def _queue_max_id(database_url):
    if not _queue_table_present(database_url):
        return None
    return db.fetchone(database_url, "SELECT COALESCE(MAX(id), 0) AS m FROM rust_job_queue")["m"]


@pytest.fixture()
def gitea_activation_world():
    """Inactive user behind a stub Gitea.

    The stub serves the token exchange plus userinfo (email included, so
    neither backend calls ``/emails``) and the emails fallback. Yields the
    world with the stub running and the ``GITEA_*`` config rows in place;
    teardown removes the config rows first (suite hermeticity), then the
    user.
    """
    database_url = config.database_url()
    store = Database(database_url)
    tag = uuid.uuid4().hex[:8]
    email = f"gh1362-{tag}@example.test"
    user = store.make_user(email, is_active=False)
    world = {"tag": tag, "email": email, "user": user}
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
                "login": f"gh1362-{tag}",
                "full_name": "Contract User",
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
            _delete_world(store, user["id"])
            raise
        try:
            world["stub"] = stub
            yield world
        finally:
            _delete_gitea_config(database_url)
            _delete_world(store, user["id"])


def test_app_gitea_activation_site_is_web_url(base_url: str, gitea_activation_world):
    """Inactive gitea login queues the activation mail with WEB_URL as site.

    The request carries ``Host: api.internal:8000`` while the backend
    booted with ``WEB_URL`` elsewhere; the queued
    ``user_activation_email`` first arg must equal ``CONTRACT_WEB_URL`` on
    both backends. Pre-fix Rust queues ``http://api.internal:8000``.
    """
    database_url = config.database_url()
    world = gitea_activation_world
    email = world["email"]
    user_id = world["user"]["id"]
    expected = env.web_base()
    assert urlsplit(expected).hostname != urlsplit(f"http://{REQUEST_HOST}").hostname, (
        f"test is vacuous: CONTRACT_WEB_URL host equals the request Host ({expected})"
    )

    broker = _redis_client_or_none()
    # The broker list is a stack for our purposes: kombu LPUSHes new
    # messages at the head, so a depth snapshot cannot locate them (an
    # index offset would miss the head and harvest stale tail entries).
    # Snapshot the entry multiset instead and diff after the drive.
    redis_before = broker.lrange("celery", 0, -1) if broker is not None else None
    queue_max_before = _queue_max_id(database_url)

    state = f"gh1362-state-{world['tag']}"
    session_key = _forge_oauth_session(database_url, state)
    new_raw = []
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
                headers={"Host": REQUEST_HOST},
            )
        finally:
            authed.close()

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
            database_url, "SELECT is_active FROM users WHERE id = %s", (user_id,)
        )
        assert user is not None and user["is_active"], "login reactivates the user"

        # Harvest exactly what this drive queued, on both channels. Only
        # one backend serves a run, so exactly one channel fires.
        sites = []
        if broker is not None:
            remaining = Counter(redis_before)
            for raw in broker.lrange("celery", 0, -1):
                if remaining[raw] > 0:
                    remaining[raw] -= 1
                else:
                    new_raw.append(raw)
            for raw in new_raw:
                task, args = _celery_task_args(raw)
                if task == ACTIVATION_TASK and args:
                    sites.append(args[0])
        new_rows = []
        if queue_max_before is not None:
            new_rows = db.fetchall(
                database_url,
                "SELECT id, task, args FROM rust_job_queue WHERE id > %s ORDER BY id",
                (queue_max_before,),
            )
            for row in new_rows:
                if row["task"] == ACTIVATION_TASK and row["args"]:
                    sites.append(row["args"][0])
    finally:
        _delete_session(database_url, session_key)
        # Channel hygiene: remove exactly what this drive queued so later
        # tests snapshot a clean world (snapshots are self-isolating
        # anyway, so a crash here cannot break a later run).
        try:
            if broker is not None and new_raw:
                for raw in new_raw:
                    broker.lrem("celery", 1, raw)
            if queue_max_before is not None:
                db.execute(
                    database_url,
                    "DELETE FROM rust_job_queue WHERE id > %s",
                    (queue_max_before,),
                )
        except Exception:
            pass

    assert len(sites) == 1, f"expected exactly one activation task, saw {sites!r}"
    assert sites[0] == expected, (
        f"activation site {sites[0]!r} != WEB_URL {expected!r}"
    )
