"""Fixtures for the app views + search contract suite (PIDASHCONV-87).

Domain D-29: 7 views routes (project views CRUD, global views CRUD,
global-view-issues list, view favorites) + 3 search routes (global search,
project issue search, entity search). Same black-box contract as the other
suites: httpx against a live server, raw SQL seeding, no Django imports.
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


@pytest.fixture()
def seed(db):
    s = Seed(db)
    yield s
    s.cleanup()


@pytest.fixture()
def secret():
    return env.contract_secret()


@pytest.fixture()
def tenant_a(seed):
    return seed.tenant()


@pytest.fixture()
def tenant_b(seed):
    return seed.tenant()


def session_headers(seed, user, password, secret):
    cookies = seed.session_cookie(user["id"], password, secret)
    return {"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())}


@pytest.fixture()
def project_a(seed, tenant_a):
    """A project in tenant A's workspace, owned by tenant A's user."""
    project_id = seed.project(tenant_a["workspace"]["id"])
    seed.project_member(
        project_id,
        tenant_a["workspace"]["id"],
        tenant_a["user"]["id"],
        role=20,
    )
    return project_id


@pytest.fixture()
def auth_a(seed, tenant_a, secret):
    return session_headers(seed, tenant_a["user"], tenant_a["user"]["password"], secret)


@pytest.fixture()
def auth_b(seed, tenant_b, secret):
    return session_headers(seed, tenant_b["user"], tenant_b["user"]["password"], secret)
