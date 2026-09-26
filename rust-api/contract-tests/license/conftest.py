# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""License / instance-console domain: fixtures over a live Django server.

Every test seeds Postgres directly and talks HTTP only. The DB is wiped
before each test; each test therefore starts from an empty console.
"""

import pytest

from _harness import Database, admin_client, anon_client
from _harness.settings import database_url, redis_url, secret_key


@pytest.fixture(scope="session")
def db():
    target = Database(database_url())
    url = redis_url()
    if url:
        import redis

        redis.Redis.from_url(url).flushdb()
    return target


@pytest.fixture(autouse=True)
def clean_db(db):
    db.reset()
    yield


@pytest.fixture
def world(db):
    admin = db.make_user("admin@example.com")
    member = db.make_user("member@example.com", first_name="Plain", last_name="Member")
    instance = db.make_instance()
    db.make_admin(instance["id"], admin["id"])
    secret = secret_key()
    return {
        "db": db,
        "admin": admin,
        "member": member,
        "instance": instance,
        "admin_key": db.mint_admin_session(admin, secret),
        "member_key": db.mint_admin_session(member, secret),
    }


@pytest.fixture
def admin_api(world):
    with admin_client(world["admin_key"]) as client:
        yield client


@pytest.fixture
def member_api(world):
    with admin_client(world["member_key"]) as client:
        yield client


@pytest.fixture
def anon_api():
    with anon_client() as client:
        yield client
