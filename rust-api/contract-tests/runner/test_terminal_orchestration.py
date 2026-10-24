"""Terminal-run orchestration behind the daemon ``complete`` endpoint.

A daemon terminal transition runs the post-run follow-ups through the api
plane's ``RunnerPorts`` implementation *before* it stamps
``terminal_hooks_applied_at`` / ``terminal_capacity_released_at``. When those
ports are warn-logged no-ops the markers are still stamped, so the worker's
live executors skip and the follow-ups are lost: the freed runner never drains
the pod's queued run, a ``stop_ticking`` done signal never disarms the ticker,
and a cap-hit deferred pause never lands. These tests drive the daemon
``complete`` endpoint and read the engine tables.

Every assertion runs against both backends: Django is the oracle, the Rust
binary must match it. On pre-fix ``rust-dev`` the follow-ups do not run, so the
drain disarms / pause assertions fail.
"""

from __future__ import annotations

import json
import uuid

import pytest

from .conftest import DAEMON

pytestmark = pytest.mark.contract


# -- seeding helpers --------------------------------------------------------


def _make_state(seeder, workspace_id, project_id, name, group):
    state_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO states
               (id, workspace_id, project_id, name, description, color, slug,
                sequence, "group", is_triage, "default", created_at, updated_at)
           VALUES (%s,%s,%s,%s,'','#808080',%s,65535,%s,false,false,now(),now())""",
        (state_id, workspace_id, project_id, name, name.lower().replace(" ", "-"), group),
    )
    seeder._put("states", state_id)
    return state_id


def _set_ticker(seeder, issue_id, **fields):
    row = {
        "id": str(uuid.uuid4()),
        "issue_id": issue_id,
        "user_disabled": False,
        "used": 0,
        "enabled": True,
        "next_run_at": None,
        "last_tick_at": None,
        "disarm_reason": "",
        "granted": 0,
        "pending_entry": False,
        "pending_entry_free": False,
        "pending_entry_actor_id": None,
        "pending_entry_trigger": "",
        "resume_parent_run_id": None,
        "waited": 0,
    }
    row.update(fields)
    cols = ",".join(row)
    vals = ",".join(["%s"] * len(row))
    seeder.db.execute(
        f"INSERT INTO issue_agent_ticker ({cols}, created_at, updated_at) "
        f"VALUES ({vals}, now(), now())",
        tuple(row.values()),
    )
    seeder._put("issue_agent_ticker", row["id"])
    return row["id"]


def _set_runner_online(seeder, runner_id):
    seeder.db.execute(
        "UPDATE runner SET status = 'online', last_heartbeat_at = now() WHERE id = %s",
        (runner_id,),
    )


def _make_online_runner(seeder, owner_id, workspace_id, pod_id):
    runner_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO runner
           (id, name, capabilities, status, os, arch, runner_version,
            protocol_version, last_heartbeat_at, created_at, updated_at,
            owner_id, workspace_id, pod_id, host_label,
            refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version, enrollment_token_hash,
            enrollment_token_fingerprint, revoked_reason, visibility,
            dev_metadata, provisioning)
           VALUES (%s,%s,'[]','online','','','',1,now(),now(),now(),%s,%s,%s,'',
                   '','',0,'',1,'','','',0,'{}','manual')""",
        (runner_id, f"handoff-{uuid.uuid4().hex[:8]}", owner_id, workspace_id, pod_id),
    )
    return runner_id


def _complete(machine_client, run_id, done_payload):
    machine_client.post(f"{DAEMON}/runs/{run_id}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{run_id}/complete/",
        json={"done_payload": done_payload},
        headers={"Idempotency-Key": uuid.uuid4().hex},
    )
    assert response.status_code == 200, response.text
    assert response.json() == {"ok": True}, response.text


# -- fixtures ---------------------------------------------------------------


@pytest.fixture()
def issue_world(seeder, daemon_world):
    """An In Progress issue (created by the runner owner) plus a Paused state.

    Both are needed by the deferred-pause gate: a ticking ``started`` state and
    a project state literally named ``Paused``.
    """
    workspace_id = daemon_world["workspace"]["id"]
    project_id = daemon_world["project"]["id"]
    in_progress = _make_state(seeder, workspace_id, project_id, "In Progress", "started")
    _make_state(seeder, workspace_id, project_id, "Paused", "backlog")
    issue = seeder.create_issue(workspace_id, project_id, in_progress, name="orch issue")
    # A second issue for the QUEUED run: ``agent_run_one_active_per_work_item``
    # allows one non-terminal run per work item, and ``run_with_issue`` already
    # holds the first.
    queued_issue = seeder.create_issue(workspace_id, project_id, in_progress, name="orch queued")
    # The runner is private: ``filter_runs_usable_by_runner`` grants it work
    # whose work item the owner created, so stamp ``created_by``.
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s WHERE id = ANY(%s)",
        (daemon_world["owner"]["id"], [issue["id"], queued_issue["id"]]),
    )
    return {
        "id": issue["id"],
        "queued_id": queued_issue["id"],
        "state": in_progress,
        "owner": daemon_world["owner"]["id"],
    }


@pytest.fixture()
def run_with_issue(seeder, daemon_world, machine_flow, issue_world):
    """A daemon run bound to the flow runner, pointed at ``issue_world``."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s WHERE id = %s",
        (issue_world["id"], run["id"]),
    )
    yield run
    seeder.db.execute("DELETE FROM run_message_dedupe WHERE run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_event WHERE agent_run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run["id"],))


# -- tests ------------------------------------------------------------------


def test_complete_drains_freed_runner_and_assigns_queued_run(
    machine_client, seeder, daemon_world, machine_flow, issue_world, run_with_issue
):
    """The completed run frees its runner; the pod's QUEUED run is assigned.

    ``drain_for_runner_by_id`` (matcher.py) picks the oldest usable QUEUED run
    in the runner's pod (personal queue first, then the pod queue).
    """
    _set_runner_online(seeder, machine_flow["runner_id"])
    queued = seeder.create_agent_run(
        daemon_world["workspace"]["id"],
        daemon_world["owner"]["id"],
        daemon_world["pod"]["id"],
        issue_world["queued_id"],
        status="queued",
    )
    _complete(machine_client, run_with_issue["id"], {"summary": "done"})

    row = seeder.db.fetchone(
        "SELECT status, runner_id FROM agent_run WHERE id = %s", (queued["id"],)
    )
    assert row["status"] == "assigned", row
    assert str(row["runner_id"]) == machine_flow["runner_id"], row


def test_complete_stop_ticking_disarms_ticker(
    machine_client, seeder, daemon_world, machine_flow, issue_world, run_with_issue
):
    """An explicit ``done_payload.stop_ticking`` stops the issue's clock."""
    _set_runner_online(seeder, machine_flow["runner_id"])
    _set_ticker(seeder, issue_world["id"], enabled=True)

    _complete(machine_client, run_with_issue["id"], {"stop_ticking": True})

    assert seeder.db.fetchval(
        "SELECT enabled FROM issue_agent_ticker WHERE issue_id = %s", (issue_world["id"],)
    ) is False


def test_complete_applies_deferred_cap_hit_pause(
    machine_client, seeder, daemon_world, machine_flow, issue_world, run_with_issue
):
    """A cap-hit disarm on a ticking issue auto-pauses it after the run ends."""
    _set_runner_online(seeder, machine_flow["runner_id"])
    # The ticker was disarmed by the timer tick that burned the last run of the
    # pool (CAP_HIT); the terminating run then applies the deferred pause.
    _set_ticker(
        seeder,
        issue_world["id"],
        enabled=False,
        disarm_reason="cap_hit",
        used=3,
        granted=3,
    )

    _complete(machine_client, run_with_issue["id"], {"summary": "done"})

    paused_state = seeder.db.fetchval(
        "SELECT id FROM states WHERE project_id = %s AND name = 'Paused'",
        (daemon_world["project"]["id"],),
    )
    assert (
        str(seeder.db.fetchval("SELECT state_id FROM issues WHERE id = %s", (issue_world["id"],)))
        == str(paused_state)
    )


def test_complete_move_handoff_drains_target_pod(
    machine_client, seeder, daemon_world, machine_flow, issue_world, run_with_issue
):
    """A terminal source run's move handoff assigns the replacement now.

    ``complete_project_move_handoff`` creates the replacement in the target
    project's pod and dispatches it; for a local-runner run dispatch leaves a
    ``PodDrain`` decision to the caller, which must drain that pod so the
    replacement does not sit QUEUED until an unrelated poll.
    """
    _set_runner_online(seeder, machine_flow["runner_id"])
    workspace_id = daemon_world["workspace"]["id"]
    target_project = seeder.create_project(workspace_id)
    target_state = seeder.create_state(
        workspace_id, target_project["id"], name="In Progress", group="started"
    )
    target_pod = seeder.create_pod(
        workspace_id, target_project["id"], name="handoff target", is_default=True
    )
    target_runner = _make_online_runner(
        seeder, daemon_world["owner"]["id"], workspace_id, target_pod["id"]
    )
    seeder.db.execute(
        "UPDATE issues SET project_id = %s, state_id = %s WHERE id = %s",
        (target_project["id"], target_state["id"], issue_world["id"]),
    )
    seeder.db.execute(
        "UPDATE agent_run SET run_config = %s::jsonb WHERE id = %s",
        (
            json.dumps(
                {
                    "_project_move_handoff": {
                        "target_project_id": target_project["id"],
                        "target_pod_id": target_pod["id"],
                    }
                }
            ),
            run_with_issue["id"],
        ),
    )

    _complete(machine_client, run_with_issue["id"], {"summary": "done"})

    replacement = seeder.db.fetchone(
        "SELECT status, runner_id, pod_id FROM agent_run"
        " WHERE parent_run_id = %s",
        (run_with_issue["id"],),
    )
    assert replacement is not None, "no replacement run created by the handoff"
    assert str(replacement["pod_id"]) == target_pod["id"], replacement
    assert replacement["status"] == "assigned", replacement
    assert str(replacement["runner_id"]) == target_runner, replacement
