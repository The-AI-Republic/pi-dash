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


# -- legacy desktop auto-pause side effects (#2910 §9) ----------------------
#
# Both endpoints apply their outcome to the caller's desktop-bundled
# machines (never to other tenants', never to new-path machines, never
# to revoked ones) and never fail the request over it. The instance
# kill switch is off in this env, so the profile outcome is
# ``engine_disabled``; CE's agent-token always answers the
# ``ManagedRunnerUnavailable`` 409, so its outcome is
# ``profile_unavailable``.


def _make_machine(owner_id, provisioning="desktop_bundled", revoked=False):
    import uuid

    from _harness.db import db_cursor

    machine_id = str(uuid.uuid4())
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO dev_machine
               (id, owner_id, host_label, label, visibility, provisioning,
                revoked_at, created_at, updated_at)
               VALUES (%s,%s,%s,%s,0,%s,%s,now(),now())""",
            (
                machine_id,
                owner_id,
                f"host-{machine_id[:8]}",
                f"host-{machine_id[:8]}",
                provisioning,
                "now()" if revoked else None,
            ),
        )
    return machine_id


def _pause_of(machine_id):
    from _harness.db import db_cursor

    with db_cursor() as cur:
        cur.execute(
            "SELECT engine_auto_pause_reason FROM dev_machine WHERE id = %s",
            (machine_id,),
        )
        return cur.fetchone()[0]


def test_agent_profile_applies_engine_disabled_to_legacy_machines(world, desktop_member):
    legacy = _make_machine(world.member.id)
    new_path = _make_machine(world.member.id, provisioning="manual")
    revoked = _make_machine(world.member.id, revoked=True)
    foreign = _make_machine(world.other_user.id)
    res = desktop_member.get(PROFILE_URL)
    assert res.status_code == 200, res.text
    assert res.json()["managed_runner_enabled"] is False  # kill switch off in this env
    assert _pause_of(legacy) == "engine_disabled"
    assert _pause_of(new_path) == ""
    assert _pause_of(revoked) == ""
    assert _pause_of(foreign) == ""


def test_agent_token_applies_profile_unavailable_to_legacy_machines(world, desktop_member):
    legacy = _make_machine(world.member.id)
    new_path = _make_machine(world.member.id, provisioning="manual")
    res = desktop_member.post("/api/users/me/ai-assistant/agent-token/")
    assert res.status_code == 409, res.text
    # CE has no desktop lane, so the seam raises ManagedRunnerUnavailable
    # (byok here); the request answers with its code and applies
    # profile_unavailable to the caller's legacy machines.
    assert res.json()["error"] == "byok_not_supported_on_desktop"
    assert _pause_of(legacy) == "profile_unavailable"
    assert _pause_of(new_path) == ""


def test_agent_profile_outcome_follows_the_agent_token_outcome(world, desktop_member):
    """Sequence pin: agent-token paused ``profile_unavailable``;
    the profile GET (kill switch off → ``engine_disabled``) then
    replaces it. (The reverse ``''``-clear recovery needs the switch
    on, so it is not observable in this env.)"""
    legacy = _make_machine(world.member.id)
    assert desktop_member.post("/api/users/me/ai-assistant/agent-token/").status_code == 409
    assert _pause_of(legacy) == "profile_unavailable"
    assert desktop_member.get(PROFILE_URL).status_code == 200
    assert _pause_of(legacy) == "engine_disabled"
