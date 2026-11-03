"""D-20 (v1_cycles_modules) fixtures: PIDASHCONV-78.

Kept in the domain directory (following app_scheduler/ precedent) so the
root conftest stays a plain import shim for every suite. Every test starts
from a freshly seeded database (``_seed`` autouse fixture): ``db.reset()``
deletes exactly the fixed-UUID seed rows and re-inserts them, so tests are
isolated without ever importing Django.
"""

import os
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from _harness import api, db  # noqa: E402


def _require_env(name):
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"contract tests require {name} to be set")
    return value


@pytest.fixture(scope="session")
def base_url():
    return _require_env("BASE_URL")


@pytest.fixture(scope="session")
def database_url():
    return _require_env("DATABASE_URL")


@pytest.fixture(autouse=True)
def _seed(database_url):
    db.reset()


@pytest.fixture()
def admin_client(base_url):
    with api.client_for(db.ADMIN_TOKEN) as client:
        yield client


@pytest.fixture()
def member_client(base_url):
    with api.client_for(db.MEMBER_TOKEN) as client:
        yield client


@pytest.fixture()
def guest_client(base_url):
    with api.client_for(db.GUEST_TOKEN) as client:
        yield client


@pytest.fixture()
def outsider_client(base_url):
    with api.client_for(db.OUTSIDER_TOKEN) as client:
        yield client


@pytest.fixture()
def anon_client(base_url):
    with api.anon_client() as client:
        yield client


@pytest.fixture()
def db_conn(database_url):
    conn = db.connect()
    try:
        yield conn
    finally:
        conn.close()
