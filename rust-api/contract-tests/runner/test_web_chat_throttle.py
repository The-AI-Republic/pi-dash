# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner chat message-send throttle (``/api/runners/chat/...``).

The message POST is braked 60/minute per user (``ChatSendThrottle``,
POST-only). The 429 carries the rewritten ``RATE_LIMIT_EXCEEDED`` body
with ``Retry-After`` = ceil(wait).
"""

from __future__ import annotations

import os

import httpx
import pytest
import redis

from _harness.auth import login_session
from _harness.throttle import reset_throttle_counters, seed_throttle_window
from _harness.web import web_post

WEB = "/api/runners"

pytestmark = pytest.mark.contract

DENIED_BODY = '{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'


def test_chat_send_throttled_retry_after_is_ceiled(
    settings, daemon_world, machine_flow, chat_world
):
    # NEWPIDASH-43: the 429 carries Retry-After = ceil(wait). The seeded
    # window (60 timestamps 0.2s old) makes the true wait ~59.8s, so DRF
    # answers '60'; a floored renderer answers '59' and a missing header
    # fails outright. The runner stays offline: throttles run before the
    # view, so the denial fires regardless of runner state.
    # Plain client: ContractClient retries 429s (sleeping Retry-After),
    # which would swallow the very response this case asserts on.
    owner = daemon_world["owner"]
    # The suite runs >30 anon hits (CSRF + sign-in per real login) and
    # would exhaust the 30/min AnonRateThrottle before this test's CSRF
    # priming. Flush the throttle counters first so the sign-in itself
    # never 429s; the runner_chat_send window is seeded after sign-in.
    reset_throttle_counters()
    rdb = redis.Redis.from_url(os.environ["REDIS_URL"], socket_timeout=5)
    with httpx.Client(
        base_url=settings.base_url, timeout=30.0, follow_redirects=True
    ) as client:
        login_session(client, email=owner["email"], password=owner["password"])
        seed_throttle_window(rdb, "runner_chat_send", owner["id"], count=60)
        try:
            resp = web_post(
                client,
                f"{WEB}/chat/sessions/{chat_world['session']['id']}/messages/",
                {"content": "hello"},
            )
        finally:
            rdb.close()
    assert resp.status_code == 429, resp.text
    assert resp.text == DENIED_BODY, resp.text
    assert resp.headers.get("retry-after") == "60", resp.headers.get("retry-after")
