# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Threads: list/create/detail contract.

Covers ``GET/POST threads/`` and ``PATCH/DELETE threads/<id>/``, including
the thread-list shape, the guest/non-member denials, tenant isolation (a
member cannot see another user's thread; loop threads stay hidden), and the
abandoned-empty-thread reaping the list performs.
"""

import uuid

from _harness.db import db_cursor

from .conftest import threads_url


def _thread_shape(t):
    assert set(t.keys()) >= {
        "id",
        "title",
        "is_archived",
        "has_active_turn",
        "created_at",
        "updated_at",
    }
    assert t["is_archived"] is False
    assert t["has_active_turn"] is False


def test_member_can_list_and_create_threads(world, member):
    base = threads_url(world)
    assert member.get(f"{base}/threads/").status_code == 200

    res = member.post(f"{base}/threads/", json={"title": "Hi"})
    assert res.status_code == 201
    body = res.json()
    _thread_shape(body)
    assert body["title"] == "Hi"

    ids = [t["id"] for t in member.get(f"{base}/threads/").json()]
    assert body["id"] in ids


def test_thread_title_is_optional_and_capped(world, member):
    base = threads_url(world)
    res = member.post(f"{base}/threads/", json={})
    assert res.status_code == 201
    assert res.json()["title"] == ""

    res = member.post(f"{base}/threads/", json={"title": "x" * 500})
    assert res.status_code == 201
    assert len(res.json()["title"]) == 255


def test_guest_is_denied_with_role_code(world, guest):
    res = guest.get(f"{threads_url(world)}/threads/")
    assert res.status_code == 403
    assert res.json()["error"] == "role_not_allowed"


def test_non_member_cannot_list_threads(world, outsider_client):
    res = outsider_client.get(f"{threads_url(world)}/threads/")
    assert res.status_code == 403


def test_anonymous_cannot_list_threads(world, anon):
    assert anon.get(f"{threads_url(world)}/threads/").status_code == 401


def test_member_cannot_touch_another_users_thread(world, member, admin):
    base = threads_url(world)
    other = admin.post(f"{base}/threads/", json={"title": "admin"}).json()

    assert member.get(f"{base}/threads/{other['id']}/messages/").status_code == 404
    assert member.patch(f"{base}/threads/{other['id']}/", json={"title": "hijack"}).status_code == 404
    assert member.delete(f"{base}/threads/{other['id']}/").status_code == 404

    # Still intact and unrenamed.
    got = admin.get(f"{base}/threads/").json()
    assert [t["title"] for t in got if t["id"] == other["id"]] == ["admin"]


def test_patch_and_delete_thread(world, member):
    base = threads_url(world)
    created = member.post(f"{base}/threads/", json={"title": "draft"}).json()

    res = member.patch(
        f"{base}/threads/{created['id']}/", json={"title": "final", "is_archived": True}
    )
    assert res.status_code == 200
    assert res.json()["title"] == "final"
    assert res.json()["is_archived"] is True

    assert member.delete(f"{base}/threads/{created['id']}/").status_code == 204
    ids = [t["id"] for t in member.get(f"{base}/threads/").json()]
    assert created["id"] not in ids


def test_patch_missing_thread_is_404(world, member):
    res = member.patch(
        f"{threads_url(world)}/threads/{uuid.uuid4()}/", json={"title": "x"}
    )
    assert res.status_code == 404


def _seed_thread(world, user_id, *, title="", kind="chat", created_at_sql="now()"):
    tid = str(uuid.uuid4())
    with db_cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_thread (id, workspace_id, user_id, title, kind,"
            " is_archived, created_at, updated_at)"
            f" VALUES (%s,%s,%s,%s,%s,false,{created_at_sql},{created_at_sql})",
            (tid, world.ws.id, user_id, title, kind),
        )
    return tid


def test_loop_threads_are_hidden_from_the_list(world, member):
    _seed_thread(world, world.member.id, title="loop run", kind="loop")
    titles = [t["title"] for t in member.get(f"{threads_url(world)}/threads/").json()]
    assert "loop run" not in titles


def test_listing_reaps_abandoned_empty_threads(world, member):
    stale = _seed_thread(
        world, world.member.id, created_at_sql="now() - interval '2 hours'"
    )
    fresh = _seed_thread(world, world.member.id)
    titled = _seed_thread(
        world, world.member.id, title="Kept", created_at_sql="now() - interval '2 hours'"
    )

    assert member.get(f"{threads_url(world)}/threads/").status_code == 200

    with db_cursor() as cur:
        cur.execute("SELECT count(*) FROM assistant_thread WHERE id=%s", (stale,))
        assert cur.fetchone()[0] == 0
        for kept in (fresh, titled):
            cur.execute("SELECT count(*) FROM assistant_thread WHERE id=%s", (kept,))
            assert cur.fetchone()[0] == 1
