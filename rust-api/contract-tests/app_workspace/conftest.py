# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-24 domain: app workspace + users + API tokens (+ timezone).

Every test seeds Postgres directly and talks HTTP only. The DB is wiped
before each test; each test therefore starts from an empty workspace.
Auth is the user `session-id` cookie (DRF SessionAuthentication, no CSRF).
"""

import pytest

from _harness import Database, anon_client, user_client
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
    # Sweep rows with FKs into projects that Database.reset() does not
    # know about (the draft-to-issue convert mints issues + sequences) so
    # the reset below never hits a foreign key — the auth_session/license
    # sweep-before-reset precedent. Drafts and favorites go before
    # projects: both carry a project FK that reset() deletes later.
    with db.connect() as conn:
        with conn.cursor() as cur:
            for table in (
                "issue_sequences",
                "issue_assignees",
                "issue_labels",
                # Activities carry FKs into projects (and issues); the graph
                # timezone suite (PIDASHCONV-1371) seeds them directly.
                "issue_activities",
                "user_favorites",
                "issues",
                "draft_issue_assignees",
                "draft_issue_cycles",
                "draft_issue_labels",
                "draft_issue_modules",
                "draft_issues",
                "states",
                "project_members",
                "projects",
            ):
                cur.execute(f"DELETE FROM {table}")
        conn.commit()
    db.reset()
    yield


@pytest.fixture
def world(db):
    user = db.make_user("user@example.com")
    other = db.make_user("other@example.com", first_name="Other", last_name="User")
    bot = db.make_user("bot@example.com", first_name="Bot", last_name="User", is_bot=True)
    # Signup creates one profile row per user with model defaults; the
    # profile/settings/onboard/tour endpoints 404 without it.
    for u in (user, other, bot):
        db.make_profile(u["id"])
    secret = secret_key()
    return {
        "db": db,
        "user": user,
        "other": other,
        "bot": bot,
        "user_key": db.mint_user_session(user, secret),
        "other_key": db.mint_user_session(other, secret),
        "bot_key": db.mint_user_session(bot, secret),
    }


@pytest.fixture
def user_api(world):
    with user_client(world["user_key"]) as client:
        yield client


@pytest.fixture
def other_api(world):
    with user_client(world["other_key"]) as client:
        yield client


@pytest.fixture
def bot_api(world):
    with user_client(world["bot_key"]) as client:
        yield client


@pytest.fixture
def anon_api():
    with anon_client() as client:
        yield client
