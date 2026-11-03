# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant DRF throttle enforcement (PIDASHCONV-1239).

DRF ``APIView.initial()`` runs ``check_throttles`` before the view body, so
the LLM-test (6/minute), STT-test (6/minute) and generate-title
(20/minute) scopes 429 the over-limit call whatever the view would have
answered, and the message-POST scope (30/hour) counts denied POSTs: a
throttled caller gets 429 before the member/thread gates. Every test uses
a fresh user (the ``world`` fixture is function-scoped), so minute/hour
windows never leak across tests or reruns.
"""

import uuid

from .conftest import threads_url

LLM_TEST_URL = "/api/users/me/ai-assistant/config/test/"
STT_TEST_URL = "/api/users/me/ai-assistant/stt-config/test/"

RATE_LIMIT_BODY = b'{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'


def _assert_throttled(res):
    assert res.status_code == 429
    assert res.content == RATE_LIMIT_BODY


def test_llm_test_throttle_trips_on_seventh_call(world, member):
    # No config: the deterministic 200 {"ok": False} gate answers until the
    # 6/minute quota is spent.
    for _ in range(6):
        res = member.post(LLM_TEST_URL, json={})
        assert res.status_code == 200
    _assert_throttled(member.post(LLM_TEST_URL, json={}))


def test_stt_test_throttle_trips_on_seventh_call(world, member):
    for _ in range(6):
        res = member.post(STT_TEST_URL, json={})
        assert res.status_code == 200
    _assert_throttled(member.post(STT_TEST_URL, json={}))


def test_generate_title_throttle_trips_on_twenty_first_call(world, member):
    url = f"/api/workspaces/{world.ws.slug}/ai-assistant/generate-title/"
    # No LLM config: the deterministic 422 gate answers until the 20/minute
    # quota is spent.
    for _ in range(20):
        res = member.post(url, json={"description": "some description"})
        assert res.status_code == 422
    _assert_throttled(member.post(url, json={"description": "some description"}))


def test_message_throttle_counts_denied_posts(world, member, admin):
    base = threads_url(world)
    res = admin.post(f"{base}/threads/", json={"title": "t"})
    assert res.status_code == 201
    unowned = res.json()["id"]

    # Denied POSTs count: 30 404s spend the 30/hour quota, the 31st is 429.
    for _ in range(30):
        res = member.post(f"{base}/threads/{unowned}/messages/", json={"content": "hi"})
        assert res.status_code == 404
    _assert_throttled(
        member.post(f"{base}/threads/{unowned}/messages/", json={"content": "hi"})
    )

    # A throttled caller gets 429 before the thread gate, even on a thread
    # id they do not own.
    _assert_throttled(
        member.post(f"{base}/threads/{uuid.uuid4()}/messages/", json={"content": "hi"})
    )

    res = member.post(f"{base}/threads/", json={"title": "mine"})
    assert res.status_code == 201
    _assert_throttled(
        member.post(f"{base}/threads/{res.json()['id']}/messages/", json={"content": "hi"})
    )
