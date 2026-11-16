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
from _harness.throttle import reset_throttle_counters


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
    # Sweep rows with FKs into users that Database.reset() does not know
    # about (the signal-created preference rows sign-up leaves behind) so
    # the reset below never hits a foreign key. Avatar/logo tests also
    # seed file_assets (referenced by users.avatar_asset_id and
    # workspaces.logo_asset_id), so those FKs are nulled and the asset
    # rows wiped before the reset deletes users/workspaces.
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM user_notification_preferences")
            cur.execute("UPDATE users SET avatar_asset_id = NULL")
            cur.execute("UPDATE workspaces SET logo_asset_id = NULL")
            cur.execute("DELETE FROM file_assets")
        conn.commit()
    db.reset()
    # The anon throttle (30/min per IP, redis-backed) is never a contract
    # answer; the suite's anon GETs burst past it over a full-dir run
    # (PIDASHCONV-1096 tipped it over), so reset counters per test.
    reset_throttle_counters()
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
