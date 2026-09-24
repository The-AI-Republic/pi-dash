"""Fixtures for the prompting contract suite (PIDASHCONV-18, D-04).

Domain: 4 prompting routes (section list, section detail PUT/DELETE,
compiled, preview) + prompt-template seed rows + scheduler-builtin
seeder. Same black-box contract as the other suites: httpx against a
live server, raw SQL seeding, no Django imports.
"""

import httpx
import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness import env
from _harness.seed import Seed


@pytest.fixture(scope="session")
def api():
    with httpx.Client(base_url=env.base_url(), timeout=15) as client:
        yield client


@pytest.fixture()
def db():
    conn = env.connect()
    try:
        yield conn
    finally:
        conn.close()


def sweep_prompting_side_rows(conn, workspaces):
    """Delete server-created rows the shared ``Seed.cleanup`` cannot see.

    ``POST /api/workspaces/`` fires the workspace-create seeder (builtin
    schedulers + bindings, workspace prompt-template rows, memberships and
    project scaffolding) and the section endpoints mint
    ``prompt_section_override`` rows — all untracked, all FK-blocking the
    tracked workspace delete. The shared harness deliberately sweeps only
    leaf tables here (other suites hang pods/pages off projects), so this
    domain owns its side rows. Children precede parents; ``workspaces``
    holds only this test's ids.
    """
    if not workspaces:
        return
    with conn.cursor() as cur:
        cur.execute(
            "DELETE FROM prompt_section_override"
            " WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM scheduler_bindings"
            " WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM schedulers WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM issue_relations WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM user_favorites WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM user_recent_visits WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM issue_comments WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM issues WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM states WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM project_members WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM projects WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM workspace_members WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )
        cur.execute(
            "DELETE FROM prompt_template"
            " WHERE workspace_id = ANY(%s)",
            (workspaces,),
        )


@pytest.fixture()
def seed(db):
    s = Seed(db)
    yield s
    sweep_prompting_side_rows(db, s.tracked_ids("workspaces"))
    s.cleanup()


@pytest.fixture()
def secret():
    return env.contract_secret()


@pytest.fixture()
def tenant_a(seed):
    """Workspace admin (role 20) + workspace."""
    return seed.tenant(role=20)


@pytest.fixture()
def tenant_b(seed):
    return seed.tenant(role=20)


@pytest.fixture()
def member_a(seed, tenant_a):
    """Non-admin member (role 15) in tenant A's workspace."""
    u = seed.user()
    seed.member(tenant_a["workspace"]["id"], u["id"], role=15)
    return {"user": u, "workspace": tenant_a["workspace"]}


def session_headers(seed, user, password, secret):
    cookies = seed.session_cookie(user["id"], password, secret)
    return {"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())}


@pytest.fixture()
def auth_a(seed, tenant_a, secret):
    return session_headers(seed, tenant_a["user"], tenant_a["user"]["password"], secret)


@pytest.fixture()
def auth_b(seed, tenant_b, secret):
    return session_headers(seed, tenant_b["user"], tenant_b["user"]["password"], secret)


@pytest.fixture()
def auth_member(seed, member_a, secret):
    return session_headers(seed, member_a["user"], member_a["user"]["password"], secret)


def write_headers(api, seed, user, password, secret):
    """Session + CSRF headers for unsafe methods.

    Prompting views use DRF's default SessionAuthentication, which
    enforces CSRF (unlike the app's BaseSessionAuthentication). This
    follows the product's own flow: GET /auth/get-csrf-token/ issues
    the token + cookie, writes carry it as X-CSRFToken.
    """
    session = seed.session_cookie(user["id"], password, secret)
    cookie = "; ".join(f"{k}={v}" for k, v in session.items())
    r = api.get("/auth/get-csrf-token/", headers={"Cookie": cookie})
    assert r.status_code == 200, r.text
    token = r.json()["csrf_token"]
    csrf_cookie = r.cookies.get("csrftoken", "")
    if csrf_cookie:
        cookie = f"{cookie}; csrftoken={csrf_cookie}"
    return {"Cookie": cookie, "X-CSRFToken": token}


@pytest.fixture()
def write_a(api, seed, tenant_a, secret):
    return write_headers(api, seed, tenant_a["user"],
                         tenant_a["user"]["password"], secret)


@pytest.fixture()
def write_b(api, seed, tenant_b, secret):
    return write_headers(api, seed, tenant_b["user"],
                         tenant_b["user"]["password"], secret)


@pytest.fixture()
def write_member(api, seed, member_a, secret):
    return write_headers(api, seed, member_a["user"],
                         member_a["user"]["password"], secret)


@pytest.fixture()
def project_a(seed, tenant_a):
    """A project in tenant A's workspace with tenant A as admin member."""
    project_id = seed.project(tenant_a["workspace"]["id"])
    seed.project_member(
        project_id,
        tenant_a["workspace"]["id"],
        tenant_a["user"]["id"],
        role=20,
    )
    return project_id


@pytest.fixture()
def issue_a(seed, tenant_a, project_a):
    """An issue in tenant A's project, in a real state."""
    state_id = seed.state(project_a, tenant_a["workspace"]["id"])
    return seed.issue(
        tenant_a["workspace"]["id"], project_a, state_id=state_id
    )


@pytest.fixture()
def binding_a(seed, tenant_a, project_a):
    """A scheduler binding in tenant A's project, owned by tenant A."""
    scheduler_id = seed.scheduler(tenant_a["workspace"]["id"])
    return seed.scheduler_binding(
        tenant_a["workspace"]["id"], project_a, scheduler_id,
        tenant_a["user"]["id"],
    )
