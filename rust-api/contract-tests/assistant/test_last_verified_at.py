# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""`last_verified_at` rendering on the BYOK LLM + STT config endpoints (PIDASHCONV-1246).

Python ``datetime.isoformat()`` prints exactly six fractional digits whenever
microseconds are nonzero, so a stored ``...05.123`` renders ``.123000``. No API
path sets ``last_verified_at`` (only the ``/test/`` probes write it, after a
live provider round-trip), so the stamp is seeded straight into Postgres and
the exact GET string is asserted on both configs.
"""

from _harness.db import db_cursor

from .conftest import PUBLIC_BASE

LLM_CONFIG_URL = "/api/users/me/ai-assistant/config/"
STT_CONFIG_URL = "/api/users/me/ai-assistant/stt-config/"

STAMP = "2026-10-18 12:00:05.123+00"
EXPECTED = "2026-10-18T12:00:05.123000+00:00"


def test_last_verified_at_keeps_trailing_microsecond_zeros(world, member):
    res = member.put(
        LLM_CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text
    res = member.put(
        STT_CONFIG_URL,
        json={
            "base_url": PUBLIC_BASE,
            "model_name": "whisper-1",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text

    with db_cursor() as cur:
        cur.execute(
            "UPDATE assistant_user_llm_config SET last_verified_at = %s WHERE user_id = %s",
            (STAMP, world.member.id),
        )
        cur.execute(
            "UPDATE assistant_user_stt_config SET last_verified_at = %s WHERE user_id = %s",
            (STAMP, world.member.id),
        )

    assert member.get(LLM_CONFIG_URL).json()["last_verified_at"] == EXPECTED
    assert member.get(STT_CONFIG_URL).json()["last_verified_at"] == EXPECTED
