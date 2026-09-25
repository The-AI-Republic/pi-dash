"""Fixtures for the app-integrations contract suite (PIDASHCONV-91).

Covers the D-33 surface: ``app/urls/integration.py`` (17 paths: GitHub PAT
connect/disconnect/status/repos, GitHub App status/install/refresh/callback/
webhook, project bind/status/toggle/unbind, generic git provider accounts and
project repository bind/toggle/unbind), ``app/urls/webhook.py`` (4 paths:
webhook CRUD, secret regenerate, logs) and ``app/urls/external.py`` (3 paths:
unsplash, project + workspace AI assistant).

Every test seeds its own world (unique tag per test) straight into Postgres
and deletes only its own rows afterwards. HTTP goes through httpx with a
session cookie: app views use ``BaseSessionAuthentication``, so each client
signs its seeded user in through the real ``/auth/sign-in/`` form endpoint
(black box) via ``_harness.auth.login_session``.

Auth clients are session-scoped (one login per role for the whole suite)
while worlds stay function-scoped: the live server throttles anonymous
requests (sign-in included) to 30/minute, so per-test logins would 429. Data
isolation is unaffected — every world uses unique slugs/rows and assertions
are scoped to the test's own workspace.

The suite runs against a live server (Django today, the Rust server through
the proxy tomorrow) selected by ``BASE_URL``; the database it seeds is
selected by ``DATABASE_URL``. The server also needs Redis (throttle cache);
``AMQP_URL`` may point at Redis or ``memory://`` (nothing here needs a
worker). The GitHub App identity keys (``GITHUB_APP_ID/SLUG/CLIENT_ID``) are
seeded into ``instance_configurations`` by the suite itself; the GitHub App
secrets, the webhook secret and ``LLM_API_KEY`` are env-sourced — set inert
dummies on the server (see the CI workflow) so the suite exercises the
configured paths instead of the 409s.
"""

from __future__ import annotations

import uuid

import httpx
import pytest

from _harness.auth import login_session
from _harness.config import get_settings
# NOTE (rebase onto rust-dev tip): the lazy fetch-helper wrapper is now
# named ``LazyDatabase`` — ``Database`` is the license suite's
# connect/reset/make_* helper with a disjoint API.
from _harness.db import LazyDatabase
from _harness.http import anonymous_client, api_client
from _harness.seed import Seeder, SeedTracker

# Workspace / project membership roles (pi_dash.app.permissions.ROLE).
ADMIN = 20
MEMBER = 15
GUEST = 5

DENIED = {"error": "You don't have the required permissions."}
ANON = {"detail": "Authentication credentials were not provided."}


@pytest.fixture(scope="session")
def ssettings():
    return get_settings()


@pytest.fixture(scope="session")
def sdb(ssettings):
    database = LazyDatabase(ssettings.database_url)
    yield database
    database.close()


@pytest.fixture(scope="session")
def sseeder(sdb):
    """Session seeder for the shared auth users (cleaned at session end).

    Per-test membership rows referencing these users are tracked by the
    function-scoped seeder and deleted first, so FK order is safe.
    """
    tracker = SeedTracker(sdb)
    seeder = Seeder(sdb, tracker, tag="sess" + uuid.uuid4().hex[:6])
    seeder.ensure_instance()
    yield seeder
    tracker.cleanup()


@pytest.fixture(scope="session")
def susers(sseeder):
    """One user per role, shared by every test (logins happen once).

    Idempotent across suite runs: the contract database is reused between
    runs, so existing rows are adopted (they were created with the default
    password) instead of re-inserted.
    """
    from _harness.seed import PASSWORD

    users = {}
    for name in ("tadmin", "tmember", "tguest", "toutsider", "tother"):
        email = f"contract91-{name}@example.com"
        row = sseeder.db.fetchone("SELECT id FROM users WHERE email = %s", (email,))
        if row is None:
            users[name] = sseeder.create_user(email=email)
        else:
            users[name] = {"id": str(row["id"]), "email": email, "password": PASSWORD}
    return users


def _login(base_url, user, *, follow_redirects=True):
    client = httpx.Client(base_url=base_url, timeout=30, follow_redirects=follow_redirects)
    return login_session(client, email=user["email"], password=user["password"])


@pytest.fixture(scope="session")
def sclients(ssettings, susers):
    """Session auth clients: admin / member / guest / outsider / other-tenant."""
    clients = {
        "admin": _login(ssettings.base_url, susers["tadmin"]),
        "member": _login(ssettings.base_url, susers["tmember"]),
        "guest": _login(ssettings.base_url, susers["tguest"]),
        "outsider": _login(ssettings.base_url, susers["toutsider"]),
        "other": _login(ssettings.base_url, susers["tother"]),
        "admin_nr": _login(ssettings.base_url, susers["tadmin"], follow_redirects=False),
    }
    yield clients
    for client in clients.values():
        client.close()


@pytest.fixture(scope="session")
def settings(ssettings):
    return ssettings


@pytest.fixture()
def db(settings):
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    seeder.ensure_instance()
    seeder.ensure_github_app_config()
    yield seeder
    tracker.cleanup()


def build_world(seeder, susers, *, tenant="main"):
    """Seed owner + workspace + project; grant the session users their roles."""
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    # NOTE (rebase onto rust-dev tip): _harness Seeder member factories take
    # the workspace first — create_workspace_member(workspace_id, user_id)
    # and create_project_member(workspace_id, project_id, user_id).
    seeder.create_workspace_member(workspace["id"], owner["id"], role=ADMIN)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=ADMIN)
    if tenant == "main":
        seeder.create_workspace_member(workspace["id"], susers["tadmin"]["id"], role=ADMIN)
        seeder.create_project_member(
            workspace["id"], project["id"], susers["tadmin"]["id"], role=ADMIN
        )
        seeder.create_workspace_member(workspace["id"], susers["tmember"]["id"], role=MEMBER)
        seeder.create_project_member(
            workspace["id"], project["id"], susers["tmember"]["id"], role=MEMBER
        )
        seeder.create_workspace_member(workspace["id"], susers["tguest"]["id"], role=GUEST)
        seeder.create_project_member(
            workspace["id"], project["id"], susers["tguest"]["id"], role=GUEST
        )
    else:
        seeder.create_workspace_member(workspace["id"], susers["tother"]["id"], role=ADMIN)
        seeder.create_project_member(
            workspace["id"], project["id"], susers["tother"]["id"], role=ADMIN
        )
    return {"owner": owner, "workspace": workspace, "project": project}


@pytest.fixture()
def world(seeder, susers):
    return build_world(seeder, susers, tenant="main")


@pytest.fixture()
def other_world(seeder, susers):
    """A second tenant: cross-tenant reads must not leak rows."""
    return build_world(seeder, susers, tenant="other")


@pytest.fixture()
def admin(sclients):
    return sclients["admin"]


@pytest.fixture()
def member_client(sclients):
    """A MEMBER-role user (workspace + project): reads pass, admin writes 403."""
    return sclients["member"]


@pytest.fixture()
def guest_client(sclients):
    """A GUEST-role user (workspace + project): only guest-allowed reads pass."""
    return sclients["guest"]


@pytest.fixture()
def outsider_client(sclients):
    """A user with no membership anywhere: every scoped endpoint 403s."""
    return sclients["outsider"]


@pytest.fixture()
def other_admin(sclients):
    return sclients["other"]


@pytest.fixture()
def admin_nr(sclients):
    """Session admin client that does not follow redirects (callback 302s)."""
    return sclients["admin_nr"]


@pytest.fixture()
def anon(settings):
    with anonymous_client(settings.base_url) as client:
        yield client


@pytest.fixture()
def no_redirect(settings):
    """Callback endpoints answer 302s; follow nothing so Location is asserted."""
    with httpx.Client(base_url=settings.base_url, timeout=30, follow_redirects=False) as client:
        yield client


def login_as(base_url, user):
    """Fresh login for tests that need a private user (e.g. user-scoped lists)."""
    return login_session(api_client(base_url), email=user["email"], password=user["password"])


def ws_url(world, *parts):
    base = f"/api/workspaces/{world['workspace']['slug']}"
    return base + "".join(f"/{p}" for p in parts) + "/"


def proj_url(world, *parts):
    base = (
        f"/api/workspaces/{world['workspace']['slug']}"
        f"/projects/{world['project']['id']}"
    )
    return base + "".join(f"/{p}" for p in parts) + "/"
