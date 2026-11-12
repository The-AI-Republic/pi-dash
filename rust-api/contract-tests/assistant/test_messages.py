# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Messages, turns and cancel: the chat write path.

Covers ``GET/POST threads/<id>/messages/`` (envelope shape, pagination,
validation errors, the ``llm_config_missing`` gate, the 202 queued-turn
shape with first-message auto-title, and the ``turn_active`` brake) and
``POST threads/<id>/cancel/``. The message POST throttle (30/hour) is a
per-user quota; every test uses fresh users, so the suite never trips it.
"""

import uuid

from _harness.db import db_cursor

from .conftest import PUBLIC_BASE, threads_url


def _configure_llm(client):
    res = client.put(
        "/api/users/me/ai-assistant/config/",
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text
    return res.json()


def _new_thread(client, base, title=""):
    res = client.post(f"{base}/threads/", json={"title": title})
    assert res.status_code == 201
    return res.json()["id"]


def _message_shape(m, *, role="user", content="hi"):
    assert set(m.keys()) >= {
        "id",
        "role",
        "content",
        "status",
        "seq",
        "turn_id",
        "payload",
        "created_at",
    }
    assert m["role"] == role
    assert m["content"] == content


def test_message_requires_llm_config(world, member):
    tid = _new_thread(member, threads_url(world))
    res = member.post(
        f"{threads_url(world)}/threads/{tid}/messages/", json={"content": "hello"}
    )
    assert res.status_code == 422
    assert res.json()["error"] == "llm_config_missing"


def test_message_post_creates_queued_turn_and_auto_titles(world, member):
    base = threads_url(world)
    _configure_llm(member)
    tid = _new_thread(member, base)

    res = member.post(f"{base}/threads/{tid}/messages/", json={"content": "do a thing"})
    assert res.status_code == 202
    body = res.json()
    assert body["turn"]["status"] == "queued"
    assert body["turn"]["id"]
    _message_shape(body["message"], content="do a thing")

    titles = {t["id"]: t["title"] for t in member.get(f"{base}/threads/").json()}
    assert titles[tid] == "do a thing"

    # A second post while the turn is in flight is rejected.
    res2 = member.post(f"{base}/threads/{tid}/messages/", json={"content": "again"})
    assert res2.status_code == 409
    assert res2.json()["error"] == "turn_active"


def test_message_validation_errors(world, member):
    base = threads_url(world)
    _configure_llm(member)
    tid = _new_thread(member, base)
    url = f"{base}/threads/{tid}/messages/"

    res = member.post(url, json={"content": "   "})
    assert res.status_code == 400
    assert res.json()["error"] == "empty_message"

    res = member.post(url, json={"content": "x" * 32001})
    assert res.status_code == 400
    assert res.json()["error"] == "message_too_long"


def test_cannot_post_to_another_users_thread(world, member, admin):
    base = threads_url(world)
    other = _new_thread(admin, base, title="admin")
    res = member.post(f"{base}/threads/{other}/messages/", json={"content": "hi"})
    assert res.status_code == 404


def test_guest_cannot_post_messages(world, guest):
    res = guest.post(
        f"{threads_url(world)}/threads/{uuid.uuid4()}/messages/",
        json={"content": "hi"},
    )
    assert res.status_code == 403


def _seed_message(thread_id, seq, content):
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO assistant_message (id, thread_id, seq, kind,
                display_content, payload, status, created_at)
               VALUES (%s,%s,%s,'user',%s,'{}','completed',now())""",
            (str(uuid.uuid4()), thread_id, seq, content),
        )


def test_message_list_envelope_and_pagination(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    for i in range(1, 4):
        _seed_message(tid, i, f"m{i}")
    url = f"{base}/threads/{tid}/messages/"

    res = member.get(url)
    assert res.status_code == 200
    msgs = res.json()
    assert [m["seq"] for m in msgs] == [1, 2, 3]
    _message_shape(msgs[0], content="m1")

    res = member.get(url, params={"after": 1, "limit": 1})
    assert [m["seq"] for m in res.json()] == [2]

    # Non-numeric cursors fall back to defaults instead of a 500.
    res = member.get(url, params={"after": "abc", "limit": "abc"})
    assert res.status_code == 200
    assert len(res.json()) == 3


def test_cancel_without_active_turn_is_409(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    res = member.post(f"{base}/threads/{tid}/cancel/")
    assert res.status_code == 409
    assert res.json()["error"] == "no_active_turn"


def test_cancel_with_active_turn_is_204(world, member):
    base = threads_url(world)
    _configure_llm(member)
    tid = _new_thread(member, base)
    assert (
        member.post(f"{base}/threads/{tid}/messages/", json={"content": "run"}).status_code
        == 202
    )
    assert member.post(f"{base}/threads/{tid}/cancel/").status_code == 204


def test_cancel_missing_thread_is_404(world, member):
    res = member.post(f"{threads_url(world)}/threads/{uuid.uuid4()}/cancel/")
    assert res.status_code == 404
