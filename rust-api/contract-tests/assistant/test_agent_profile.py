# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop agent-profile body (PIDASHCONV-1568).

``GET /api/users/me/ai-assistant/agent-profile/`` as a desktop session
serves the model profile; CE carries an empty ``top_up_url`` (no credit
lane — the overlay fills the URL in the cloud build). The key order pins
the Django dict order so the Rust body stays byte-identical.
"""

PROFILE_URL = "/api/users/me/ai-assistant/agent-profile/"

PROFILE_KEYS = [
    "managed_runner_enabled",
    "lane",
    "base_url",
    "model",
    "top_up_url",
    "available",
    "reason_code",
    "graceful_stop_seconds",
]


def test_agent_profile_carries_top_up_url(world, desktop_member):
    res = desktop_member.get(PROFILE_URL)
    assert res.status_code == 200
    body = res.json()
    assert list(body.keys()) == PROFILE_KEYS
    assert body["top_up_url"] == ""
    # Keyless world member: the no-config arm, whatever the instance switch.
    assert body["lane"] == ""
    assert body["available"] is False
    assert body["reason_code"] == "llm_config_missing"
