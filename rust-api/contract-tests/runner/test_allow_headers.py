"""Runner J10 journey Allow trace (PIDASHCONV-1048).

The stage-6 gate's runner journey — register → session → dispatch from
issue → accept/started/complete → final — must carry DRF-identical
``Allow`` on every step (Django stamps it on all view-rendered
responses via ``finalize_response``). The per-endpoint suites pin
individual arms; this test walks the journey once and pins the header
on each step's response, plus list/health/metrics.
"""

from __future__ import annotations

import pytest

from _harness.http import api_key_client, bearer_client
from _harness.web import web_post

from .conftest import DAEMON

WEB = "/api/runners"

pytestmark = pytest.mark.contract

ALLOW_RUNS = "GET, POST, HEAD, OPTIONS"
ALLOW_GET = "GET, HEAD, OPTIONS"
ALLOW_POST = "POST, OPTIONS"


def test_runner_journey_allow_trace(settings, seeder, daemon_world, user_client, anon_client):
    host_label = f"j10allow-{seeder.tag}"[:200]
    # 1. Register (CLI create): 201 + POST-only Allow.
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        registered = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host_label,
            },
        )
    assert registered.status_code == 201, registered.text
    assert registered.headers["allow"] == ALLOW_POST
    runner_id = registered.json()["runner_id"]
    machine_token = registered.json()["machine_token"]
    run_id = None
    try:
        with bearer_client(
            settings.base_url, machine_token, headers={"X-Runner-Id": runner_id}
        ) as daemon:
            # 2. Session open: 201 + POST-only Allow.
            opened = daemon.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
            assert opened.status_code == 201, opened.text
            assert opened.headers["allow"] == ALLOW_POST
            # 3. Dispatch from issue (run_ai tail): 201 + list-view Allow.
            state = seeder.create_state(
                daemon_world["workspace"]["id"], daemon_world["project"]["id"],
                name="In Progress", group="started",
            )
            issue = seeder.create_issue(
                daemon_world["workspace"]["id"], daemon_world["project"]["id"], state["id"]
            )
            # Real issues have a creator; the seeder leaves created_by NULL,
            # which trips an unrelated pre-existing Rust gap (run_detail
            # 500s on NULL-created_by rows while Django 200s — filed
            # separately, out of Allow scope here).
            seeder.db.execute(
                "UPDATE issues SET created_by_id = %s WHERE id = %s",
                (daemon_world["owner"]["id"], issue["id"]),
            )
            dispatched = web_post(
                user_client, f"{WEB}/runs/",
                {"triggered_by": "run_ai", "work_item": issue["id"]},
            )
            assert dispatched.status_code == 201, dispatched.text
            assert dispatched.headers["allow"] == ALLOW_RUNS
            run_id = dispatched.json()["id"]
            # The lifecycle legs below mirror the daemon_run fixture: an
            # ASSIGNED run owned by the journey runner.
            seeder.db.execute(
                "UPDATE agent_run SET runner_id = %s, status = 'assigned' WHERE id = %s",
                (runner_id, run_id),
            )
            # 4-6. accept → started → complete, each POST-only Allow.
            accepted = daemon.post(f"{DAEMON}/runs/{run_id}/accept/", json={})
            assert accepted.json() == {"ok": True}
            assert accepted.headers["allow"] == ALLOW_POST
            started = daemon.post(
                f"{DAEMON}/runs/{run_id}/started/", json={"thread_id": "thr-j10allow"}
            )
            assert started.json() == {"ok": True}
            assert started.headers["allow"] == ALLOW_POST
            completed = daemon.post(
                f"{DAEMON}/runs/{run_id}/complete/",
                json={"done_payload": {"summary": "done"}},
            )
            assert completed.json() == {"ok": True}
            assert completed.headers["allow"] == ALLOW_POST
        # 7. Final (run detail): GET-only Allow.
        final = user_client.get(f"{WEB}/runs/{run_id}/")
        assert final.status_code == 200, final.text
        assert final.headers["allow"] == ALLOW_GET
        assert final.json()["status"] == "completed"
        # 8. List + daemon probes.
        listed = user_client.get(f"{WEB}/runs/")
        assert listed.status_code == 200, listed.text
        assert listed.headers["allow"] == ALLOW_RUNS
        health = anon_client.get(f"{DAEMON}/health/")
        assert health.status_code == 200
        assert health.headers["allow"] == ALLOW_GET
        metrics = anon_client.get(f"{DAEMON}/metrics/")
        assert metrics.status_code == 200
        assert metrics.headers["allow"] == ALLOW_GET
    finally:
        # Teardown the endpoint-minted rows (the machine_flow mirror).
        owner_id = daemon_world["owner"]["id"]
        if run_id is not None:
            seeder.db.execute(
                "DELETE FROM run_message_dedupe WHERE run_id = %s", (run_id,)
            )
            seeder.db.execute(
                "DELETE FROM agent_run_event WHERE agent_run_id = %s", (run_id,)
            )
            seeder.db.execute(
                "DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run_id,)
            )
            seeder.db.execute("DELETE FROM agent_run WHERE id = %s", (run_id,))
        seeder.db.execute("DELETE FROM runner_session WHERE runner_id = %s", (runner_id,))
        seeder.db.execute(
            "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = NULL "
            "WHERE runner_id = %s OR pinned_runner_id = %s",
            (runner_id, runner_id),
        )
        seeder.db.execute(
            "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
            (owner_id, host_label),
        )
        seeder.db.execute("DELETE FROM runner WHERE id = %s", (runner_id,))
        seeder.db.execute(
            "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
            (owner_id, host_label),
        )
