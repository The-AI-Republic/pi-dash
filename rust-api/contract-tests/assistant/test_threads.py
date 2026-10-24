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


def _seed_used_thread(world, thread_id, user_id):
    """Give a thread a turn, a user message, an event and a loop target.

    Mirrors the Django schema (``assistant_turn`` / ``assistant_message`` /
    ``assistant_event`` / ``loop_jobs`` / ``loop_targets``); one transaction,
    so the deferred FK checks see a consistent graph (PIDASHCONV-1237).
    """
    turn_id = str(uuid.uuid4())
    msg_id = str(uuid.uuid4())
    job_id = str(uuid.uuid4())
    target_id = str(uuid.uuid4())
    slug = f"ct-1237-{uuid.uuid4().hex[:8]}"
    with db_cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_turn (id, thread_id, user_message_id, status,"
            " model_used, error_code, error_detail, created_at, started_at)"
            " VALUES (%s,%s,%s,'queued','','','',now(),now())",
            (turn_id, thread_id, msg_id),
        )
        cur.execute(
            "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind,"
            " display_content, payload, status, created_at)"
            " VALUES (%s,%s,%s,0,'user','do a thing','{}','completed',now())",
            (msg_id, thread_id, turn_id),
        )
        cur.execute(
            "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id,"
            " payload, created_at)"
            " VALUES (%s,%s,0,'turn.queued',%s,'{}',now())",
            (thread_id, turn_id, msg_id),
        )
        cur.execute(
            "UPDATE assistant_thread SET active_turn_id=%s WHERE id=%s",
            (turn_id, thread_id),
        )
        cur.execute(
            "INSERT INTO loop_jobs (id, slug, name, public_name, public_description,"
            " prompt, min_role, enabled, is_builtin, dtstart, rrule, tzid,"
            " created_at, updated_at)"
            " VALUES (%s,%s,'Admin name','Job','seeded','Do the thing.',"
            " 15,true,false,now(),'FREQ=DAILY;BYHOUR=3;BYMINUTE=0','UTC',now(),now())",
            (job_id, slug),
        )
        cur.execute(
            "INSERT INTO loop_targets (id, job_id, workspace_id, user_id, thread_id,"
            " next_run_at, last_run_id, last_skipped_at, last_skip_reason,"
            " created_at, updated_at)"
            " VALUES (%s,%s,%s,%s,%s,NULL,%s,NULL,'',now(),now())",
            (target_id, job_id, world.ws.id, user_id, thread_id, turn_id),
        )
    return {
        "turn_id": turn_id,
        "message_id": msg_id,
        "target_id": target_id,
    }


def test_delete_thread_with_turns_cascades_and_nulls_loop_target(world, member):
    base = threads_url(world)
    created = member.post(f"{base}/threads/", json={"title": "used"}).json()
    tid = created["id"]
    seeded = _seed_used_thread(world, tid, world.member.id)

    res = member.delete(f"{base}/threads/{tid}/")
    assert res.status_code == 204, res.text

    with db_cursor() as cur:
        for table, col, val in (
            ("assistant_thread", "id", tid),
            ("assistant_turn", "thread_id", tid),
            ("assistant_message", "thread_id", tid),
            ("assistant_event", "thread_id", tid),
        ):
            cur.execute(f"SELECT count(*) FROM {table} WHERE {col}=%s", (val,))
            assert cur.fetchone()[0] == 0, table
        cur.execute(
            "SELECT thread_id, last_run_id FROM loop_targets WHERE id=%s",
            (seeded["target_id"],),
        )
        row = cur.fetchone()
        assert row is not None
        assert row[0] is None
        assert row[1] is None


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
