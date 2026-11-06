"""Engine machine identity: rename, daemon machine, replace, merge (#2910/#2912).

Ports of ``test_engine_identity.py`` (engine runner + naming + cap),
``test_engine_machine_merge.py`` (merge) and the daemon-machine cases of
``test_engine_followups.py``. Dual-half: every assertion runs against
Django (oracle) and the Rust binary with the same seed + requests.
"""

from __future__ import annotations

import pytest

from _harness.http import anonymous_client, api_key_client, bearer_client

from .conftest import DAEMON

WEB = "/api/runners"
pytestmark = pytest.mark.contract

MACHINE_KEYS = [
    "id", "host_label", "label", "visibility", "provisioning",
    "runner_count", "online_runner_count", "control_online",
    "engine_paused_by_owner", "engine_auto_pause_reason", "engine_paused",
    "engine_team_runs", "engine_sign_in_state", "engine_credential_expires_at",
    "engine_state", "engine_state_reported_at", "merged_into",
    "last_seen_at", "last_heartbeat_at", "revoked_at",
    "created_at", "updated_at",
]
MINI_KEYS = [
    "id", "host_label", "label", "engine_paused_by_owner",
    "engine_auto_pause_reason", "engine_team_runs", "engine_sign_in_state",
    "engine_credential_expires_at",
]
RUNNER_KEYS = [
    "id", "name", "status", "host_label", "is_engine", "engine_paused",
    "dispatch_paused", "machine_display_name", "provisioning",
    "pod_dispatch", "os", "arch", "runner_version", "dev_metadata",
    "protocol_version", "capabilities", "last_heartbeat_at", "owner",
    "dev_machine", "dev_machine_detail", "visibility", "pod",
    "pod_detail", "live_state", "enrolled_at", "revoked_at",
    "revoked_reason", "paused_at", "paused_by", "created_at",
    "updated_at",
]


def _legacy_machine(seeder, owner_id, host_label):
    machine = seeder.create_dev_machine(owner_id, host_label=host_label, provisioning="desktop_bundled")
    return machine


def _manual_machine(seeder, owner_id, host_label):
    return seeder.create_dev_machine(owner_id, host_label=host_label, provisioning="manual")


def _mt(seeder, owner_id, workspace_id, dev_machine_id, host_label):
    # Distinct raw per machine (same raw => same token_hash => unique clash).
    raw = "mt_" + dev_machine_id.replace("-", "")[:28]
    token = seeder.create_machine_token(
        owner_id, workspace_id, raw, host_label=host_label, dev_machine_id=dev_machine_id,
    )
    return raw, token


# -- web PATCH rename (label + controls) ----------------------------------

def test_web_patch_machine_label(user_client, daemon_world, seeder):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"web-patch-{seeder.tag}")
    seeder.db.execute(
        "INSERT INTO machine_token (id, user_id, workspace_id, dev_machine_id, host_label, token_hash, token_fingerprint, label, is_service, created_at) VALUES (%s,%s,%s,%s,%s,%s,%s,'t',true,now())",
        (__import__("uuid").uuid4(), daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"], f"h-{machine['id'][:8]}-{seeder.tag}", machine["id"][:8]),
    )
    response = user_client.patch(
        f"{WEB}/dev-machines/{machine['id']}/",
        json={"workspace": daemon_world["workspace"]["id"], "label": "My   Box"},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert list(body) == MACHINE_KEYS
    assert body["label"] == "My Box"
    assert body["host_label"] == machine["host_label"]


def test_web_patch_machine_unknown_key_400(user_client, daemon_world, seeder):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"web-unk-{seeder.tag}")
    seeder.db.execute(
        "INSERT INTO machine_token (id, user_id, workspace_id, dev_machine_id, host_label, token_hash, token_fingerprint, label, is_service, created_at) VALUES (%s,%s,%s,%s,%s,%s,%s,'t',true,now())",
        (__import__("uuid").uuid4(), daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"], f"h-{machine['id'][:8]}-{seeder.tag}", machine["id"][:8]),
    )
    response = user_client.patch(
        f"{WEB}/dev-machines/{machine['id']}/",
        json={"workspace": daemon_world["workspace"]["id"], "bogus": 1},
    )
    assert response.status_code == 400
    assert response.json() == {"error": "only these fields can be changed here: engine_paused_by_owner, engine_team_runs, label"}


def test_web_patch_machine_label_errors(user_client, daemon_world, seeder):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"web-lbl-{seeder.tag}")
    seeder.db.execute(
        "INSERT INTO machine_token (id, user_id, workspace_id, dev_machine_id, host_label, token_hash, token_fingerprint, label, is_service, created_at) VALUES (%s,%s,%s,%s,%s,%s,%s,'t',true,now())",
        (__import__("uuid").uuid4(), daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"], f"h-{machine['id'][:8]}-{seeder.tag}", machine["id"][:8]),
    )
    ws = daemon_world["workspace"]["id"]
    bad_type = user_client.patch(f"{WEB}/dev-machines/{machine['id']}/", json={"workspace": ws, "label": 7})
    assert bad_type.status_code == 400
    assert bad_type.json() == {"error": "label must be a string"}
    too_long = user_client.patch(f"{WEB}/dev-machines/{machine['id']}/", json={"workspace": ws, "label": "x" * 129})
    assert too_long.status_code == 400
    assert too_long.json() == {"error": "label must be at most 128 characters"}
    bad_bool = user_client.patch(f"{WEB}/dev-machines/{machine['id']}/", json={"workspace": ws, "engine_team_runs": "yes"})
    assert bad_bool.status_code == 400
    assert bad_bool.json() == {"error": "engine_team_runs must be a boolean"}


def test_web_patch_machine_empty_label_resets(user_client, daemon_world, seeder):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"web-reset-{seeder.tag}")
    seeder.db.execute(
        "UPDATE dev_machine SET label = 'Old' WHERE id = %s", (machine["id"],),
    )
    seeder.db.execute(
        "INSERT INTO machine_token (id, user_id, workspace_id, dev_machine_id, host_label, token_hash, token_fingerprint, label, is_service, created_at) VALUES (%s,%s,%s,%s,%s,%s,%s,'t',true,now())",
        (__import__("uuid").uuid4(), daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"], f"h-{machine['id'][:8]}-{seeder.tag}", machine["id"][:8]),
    )
    response = user_client.patch(
        f"{WEB}/dev-machines/{machine['id']}/",
        json={"workspace": daemon_world["workspace"]["id"], "label": ""},
    )
    assert response.status_code == 200, response.text
    assert response.json()["label"] == machine["host_label"][:128]


def test_web_patch_machine_workspace_gate(user_client, daemon_world):
    response = user_client.patch(
        f"{WEB}/dev-machines/{daemon_world['workspace']['id']}/",
        json={"label": "x"},
    )
    assert response.status_code == 400
    assert response.json() == {"error": "workspace is required"}


# -- daemon GET/PATCH ------------------------------------------------------

def test_daemon_machine_get_shape(settings, seeder, daemon_world):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"daemon-get-{seeder.tag}")
    raw, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"])
    with bearer_client(settings.base_url, raw) as client:
        response = client.get(f"{DAEMON}/dev-machines/{machine['id']}/")
    assert response.status_code == 200, response.text
    body = response.json()
    # Unannotated: no runner_count/online/last_heartbeat keys, control false.
    assert "runner_count" not in body
    assert "online_runner_count" not in body
    assert "last_heartbeat_at" not in body
    assert body["control_online"] is False
    assert body["id"] == machine["id"]
    assert "runners" in body and isinstance(body["runners"], list)
    for key in MACHINE_KEYS:
        if key in ("runner_count", "online_runner_count", "last_heartbeat_at"):
            continue
        assert key in body, key


def test_daemon_machine_get_restricted_to_token_machine(settings, seeder, daemon_world):
    machine_a = _manual_machine(seeder, daemon_world["owner"]["id"], f"daemon-a-{seeder.tag}")
    machine_b = _manual_machine(seeder, daemon_world["owner"]["id"], f"daemon-b-{seeder.tag}")
    raw_a, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine_a["id"], machine_a["host_label"])
    # Update host_label for unique token lookup not needed; token bound to A.
    with bearer_client(settings.base_url, raw_a) as client:
        response = client.get(f"{DAEMON}/dev-machines/{machine_b['id']}/")
    assert response.status_code == 403
    assert response.json() == {"error": "dev_machine_mismatch"}


def test_daemon_machine_patch_pause(settings, seeder, daemon_world):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"daemon-patch-{seeder.tag}")
    raw, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"])
    with bearer_client(settings.base_url, raw) as client:
        response = client.patch(
            f"{DAEMON}/dev-machines/{machine['id']}/",
            json={"engine_paused_by_owner": True, "label": "New Label"},
        )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["engine_paused_by_owner"] is True
    assert body["label"] == "New Label"
    assert "runners" in body


def test_daemon_patch_answers_get_body(settings, seeder, daemon_world):
    machine = _manual_machine(seeder, daemon_world["owner"]["id"], f"daemon-same-{seeder.tag}")
    raw, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], machine["id"], machine["host_label"])
    with bearer_client(settings.base_url, raw) as client:
        patched = client.patch(f"{DAEMON}/dev-machines/{machine['id']}/", json={"label": "Same"})
        assert patched.status_code == 200, patched.text
        fetched = client.get(f"{DAEMON}/dev-machines/{machine['id']}/")
        assert fetched.status_code == 200
    assert patched.json() == fetched.json()


# -- runner list includes engine ------------------------------------------

def test_runner_list_includes_engine(settings, user_client, seeder, daemon_world):
    machine = _legacy_machine(seeder, daemon_world["owner"]["id"], f"list-engine-{seeder.tag}")
    # Seed one engine (desktop_bundled) runner on the machine.
    runner = seeder.enroll_runner(daemon_world["owner"]["id"], daemon_world["workspace"]["id"], daemon_world["pod"]["id"], f"e-{seeder.tag}-{machine['id'][:8]}", name=f"engine-{seeder.tag}")
    seeder.db.execute("UPDATE runner SET provisioning='desktop_bundled', pod_dispatch='pinned_only', dev_machine_id=%s WHERE id = %s", (machine["id"], runner["id"]))
    response = user_client.get(f"{WEB}/", params={"workspace": daemon_world["workspace"]["id"]})
    assert response.status_code == 200, response.text
    names = [r["name"] for r in response.json()]
    assert f"engine-{seeder.tag}" in names
    # ?include_bundled is ignored (engine stays listed either way).
    with_include = user_client.get(
        f"{WEB}/", params={"workspace": daemon_world["workspace"]["id"], "include_bundled": "0"}
    )
    assert with_include.status_code == 200
    assert f"engine-{seeder.tag}" in [r["name"] for r in with_include.json()]


# -- replace candidates ----------------------------------------------------

def test_replace_candidates(settings, seeder, daemon_world):
    host = f"replace-{seeder.tag}"
    current = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    candidate = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    other = _manual_machine(seeder, daemon_world["owner"]["id"], f"other-{seeder.tag}")
    # Current token bound to current machine.
    raw, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], current["id"], host)
    with bearer_client(settings.base_url, raw) as client:
        response = client.get(f"{DAEMON}/dev-machines/{current['id']}/replace-candidates/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert "candidates" in body
    ids = [c["id"] for c in body["candidates"]]
    assert candidate["id"] in ids
    assert other["id"] not in ids
    assert current["id"] not in ids
    for item in body["candidates"]:
        assert set(item) == {"id", "label", "host_label", "display_name", "last_seen_at", "created_at", "provisioning", "live_runner_count"}


def test_replace_candidates_stale_session_does_not_block(settings, seeder, daemon_world):
    host = f"stale-{seeder.tag}"
    current = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    candidate = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    # Stale live-looking session (old last_seen) must not block.
    seeder.db.execute(
        "INSERT INTO machine_session (id, dev_machine_id, protocol_version, created_at, last_seen_at, revoked_reason) VALUES (%s,%s,1, now() - interval '20 minutes', now() - interval '10 minutes', '')",
        (str(__import__("uuid").uuid4()), candidate["id"]),
    )
    raw, _ = _mt(seeder, daemon_world["owner"]["id"], daemon_world["workspace"]["id"], current["id"], host)
    with bearer_client(settings.base_url, raw) as client:
        response = client.get(f"{DAEMON}/dev-machines/{current['id']}/replace-candidates/")
    assert response.status_code == 200, response.text
    assert candidate["id"] in [c["id"] for c in response.json()["candidates"]]


# -- merge -----------------------------------------------------------------

def test_merge_moves_and_collision(settings, user_client, seeder, daemon_world):
    host = f"merge-{seeder.tag}"
    source = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    target = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    pod = daemon_world["pod"]
    # Target survivor engine runner (same project via same pod).
    survivor = seeder.enroll_runner(daemon_world["owner"]["id"], daemon_world["workspace"]["id"], pod["id"], f"s-{seeder.tag}-{target['id'][:8]}", name=f"survivor-{seeder.tag}")
    survivor_id = survivor["id"]
    seeder.db.execute("UPDATE runner SET provisioning='desktop_bundled', pod_dispatch='pinned_only', dev_machine_id=%s WHERE id = %s", (target["id"], survivor_id))
    # Source loser engine runner + one manual runner to move.
    loser = seeder.enroll_runner(daemon_world["owner"]["id"], daemon_world["workspace"]["id"], pod["id"], f"l-{seeder.tag}-{source['id'][:8]}", name=f"loser-{seeder.tag}")
    loser_id = loser["id"]
    seeder.db.execute("UPDATE runner SET provisioning='desktop_bundled', pod_dispatch='pinned_only', dev_machine_id=%s WHERE id = %s", (source["id"], loser_id))
    mover = seeder.enroll_runner(daemon_world["owner"]["id"], daemon_world["workspace"]["id"], pod["id"], f"m-{seeder.tag}-{source['id'][:8]}", name=f"mover-{seeder.tag}")
    mover_id = mover["id"]
    seeder.db.execute("UPDATE runner SET dev_machine_id=%s WHERE id = %s", (source["id"], mover_id))
    # Session auth merge (cookie session via user_client).
    response = user_client.post(
        f"{DAEMON}/dev-machines/{source['id']}/merge-into/",
        json={"target_dev_machine_id": target["id"]},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["source_dev_machine_id"] == source["id"]
    assert body["target_dev_machine_id"] == target["id"]
    assert set(body) == {"source_dev_machine_id", "target_dev_machine_id", "merged_at", "moved_runner_ids", "collisions"}
    assert mover_id in body["moved_runner_ids"]
    assert any(c["revoked_runner_id"] == loser_id and c["survivor_runner_id"] == survivor_id for c in body["collisions"])
    # Moved runner now on target, offline.
    row = seeder.db.fetchone("SELECT dev_machine_id, status FROM runner WHERE id = %s", (mover_id,))
    assert (str(row["dev_machine_id"]), row["status"]) == (target["id"], "offline")


def test_merge_refusals(settings, user_client, seeder, daemon_world):
    host = f"refuse-{seeder.tag}"
    source = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    target = _manual_machine(seeder, daemon_world["owner"]["id"], host)
    # Missing target.
    missing = user_client.post(f"{DAEMON}/dev-machines/{source['id']}/merge-into/", json={})
    assert missing.status_code == 400, missing.text
    assert missing.json() == {"error": "target_dev_machine_id is required"}
    # Bad UUID.
    bad = user_client.post(f"{DAEMON}/dev-machines/{source['id']}/merge-into/", json={"target_dev_machine_id": "nope"})
    assert bad.status_code == 400, bad.text
    assert bad.json() == {"error": "invalid_target_dev_machine_id"}
    # Self merge.
    self_merge = user_client.post(
        f"{DAEMON}/dev-machines/{source['id']}/merge-into/",
        json={"target_dev_machine_id": source["id"]},
    )
    assert self_merge.status_code == 400, self_merge.text
    assert self_merge.json() == {"error": "cannot_merge_into_self"}
    # Unauthenticated (anon client).
    with anonymous_client(settings.base_url) as anon:
        unauth = anon.post(
            f"{DAEMON}/dev-machines/{source['id']}/merge-into/",
            json={"target_dev_machine_id": target["id"]},
        )
        assert unauth.status_code in (401, 403), unauth.text


def test_merge_needs_authentication(settings, daemon_world):
    with anonymous_client(settings.base_url) as anon:
        response = anon.post(
            f"{DAEMON}/dev-machines/{__import__('uuid').uuid4()}/merge-into/",
            json={"target_dev_machine_id": str(__import__("uuid").uuid4())},
        )
        assert response.status_code in (401, 403), response.text


# -- engine create (cap gone, uniqueness via legacy) ----------------------

def test_legacy_engine_create_works(settings, daemon_world, seeder):
    host = f"legacy-create-{seeder.tag}"
    _legacy_machine(seeder, daemon_world["owner"]["id"], host)
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host,
                "agent_kind": "engine",
                "name": f"legacy-{seeder.tag}",
            },
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert body["runner_name"] == f"legacy-{seeder.tag}"
    assert body["created"] is True
    seeder.db.execute("DELETE FROM machine_token WHERE user_id = %s AND host_label = %s", (daemon_world["owner"]["id"], host))
    seeder.db.execute("DELETE FROM runner WHERE id = %s", (body["runner_id"],))
    seeder.db.execute("DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s", (daemon_world["owner"]["id"], host))


def test_engine_unavailable_on_ce(settings, daemon_world, seeder):
    host = f"unavail-{seeder.tag}"
    _manual_machine(seeder, daemon_world["owner"]["id"], host)
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host,
                "agent_kind": "engine",
            },
        )
    assert response.status_code == 409, response.text
    body = response.json()
    assert body["error"] == "engine_unavailable"
    assert body["reason_code"] in ("engine_disabled", "profile_unavailable")
    assert body["error_description"] == "This Pi Dash instance cannot serve the built-in engine for this account."


def test_cap_gone(settings, daemon_world, seeder):
    # Managed cap 409 is gone: creating many manual runners never hits
    # managed_runner_limit.
    for i in range(3):
        host = f"cap-{seeder.tag}-{i}"
        with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
            response = client.post(
                f"{DAEMON}/runners/",
                json={
                    "project": daemon_world["project"]["identifier"],
                    "workspace_slug": daemon_world["workspace"]["slug"],
                    "host_label": host,
                    "name": f"cap-runner-{seeder.tag}-{i}",
                },
            )
        assert response.status_code == 201, response.text
        assert response.json()["error"] != "managed_runner_limit" if "error" in response.json() else True
        seeder.db.execute("DELETE FROM machine_token WHERE user_id = %s AND host_label = %s", (daemon_world["owner"]["id"], host))
        seeder.db.execute("DELETE FROM runner WHERE id = %s", (response.json()["runner_id"],))
        seeder.db.execute("DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s", (daemon_world["owner"]["id"], host))
