# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-287: profile user-issues legacy date filters truncate in the actor zone.

Django's ``created_at__date`` / ``updated_at__date`` / ``completed_at__date``
are ``TruncDate`` in the ``TimezoneMixin``-activated ``user_timezone``, so
``(col AT TIME ZONE '<user tz>')::date`` decides the match. Rust
``profile_filtered_set`` truncated in UTC, diverging from Django.

2026-11-13T20:00:00Z is 2026-11-14 10:00 in Pacific/Kiritimati (UTC+14):
the pinned issue matches ``created_at=2026-11-14;after`` and misses
``created_at=2026-11-13;before`` in Kiritimati, while a UTC truncation
does the opposite.
"""

import pytest

from _harness import env
from _harness.seed import Seed

pytestmark = pytest.mark.contract

PINNED_AT = "2026-11-13T20:00:00+00:00"
ACTOR_TZ = "Pacific/Kiritimati"
# Local (Kiritimati) tomorrow vs UTC today for the pinned instant.
TOMORROW = "2026-11-14"
TODAY = "2026-11-13"


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


def _set_viewer_timezone(db, user_id, tz):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "update users set user_timezone=%s where id=%s",
                (tz, user_id),
            )
        conn.commit()


def _pin_issue_dates(conn, issue_id):
    with conn.cursor() as cur:
        cur.execute(
            "update issues set created_at=%s, updated_at=%s, completed_at=%s where id=%s",
            (PINNED_AT, PINNED_AT, PINNED_AT, issue_id),
        )
    conn.commit()


def _ids(body):
    results = body.get("results", [])
    # Flat list: results is a list of rows; grouped is a dict — this test
    # only uses the flat path.
    assert isinstance(results, list), body
    return {row["id"] for row in results}


def _seed_kiritimati_profile_issue(world, space):
    _set_viewer_timezone(world["db"], world["user"]["id"], ACTOR_TZ)
    # Target users must be workspace members for the viewer-gated list.
    world["db"].make_workspace_member(
        space["id"], world["bot"]["id"], role=15,
    )
    conn = env.connect()
    try:
        seed = Seed(conn)
        uid, ws_id = world["user"]["id"], space["id"]
        bot_id = world["bot"]["id"]
        project_id = seed.project(ws_id)
        seed.project_member(project_id, ws_id, uid, role=20)
        issue_id = seed.issue(ws_id, project_id, name="287 Kiritimati pinned issue")
        # Bot assigned → in bot's profile scope (assignee leg).
        seed.issue_assignee(ws_id, project_id, issue_id, bot_id)
        _pin_issue_dates(conn, issue_id)
        return issue_id
    finally:
        conn.close()


def _profile_url(space, bot_id):
    return f"/api/workspaces/{space['slug']}/user-issues/{bot_id}/"


def test_profile_legacy_created_at_uses_actor_timezone(user_api, space, world):
    issue_id = _seed_kiritimati_profile_issue(world, space)
    bot_id = world["bot"]["id"]
    base = _profile_url(space, bot_id)
    # Django: local date 2026-11-14 is >= 2026-11-14, so the pinned row is included.
    r = user_api.get(f"{base}?created_at={TOMORROW};after")
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    # Local date 2026-11-14 is not <= 2026-11-13, so excluded.
    r = user_api.get(f"{base}?created_at={TODAY};before")
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()


def test_profile_legacy_updated_at_uses_actor_timezone(user_api, space, world):
    issue_id = _seed_kiritimati_profile_issue(world, space)
    bot_id = world["bot"]["id"]
    base = _profile_url(space, bot_id)
    r = user_api.get(f"{base}?updated_at={TOMORROW};after")
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    r = user_api.get(f"{base}?updated_at={TODAY};before")
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()


def test_profile_legacy_completed_at_uses_actor_timezone(user_api, space, world):
    issue_id = _seed_kiritimati_profile_issue(world, space)
    bot_id = world["bot"]["id"]
    base = _profile_url(space, bot_id)
    r = user_api.get(f"{base}?completed_at={TOMORROW};after")
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    r = user_api.get(f"{base}?completed_at={TODAY};before")
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()
