"""Engine dispatch core (PIDESKAPP-123, revision 4.0 sections 6-9).

Black-box port of Django's ``pi_dash/tests/contract/runner/
test_engine_dispatch.py``: the single Runner executor with pod-level
translation of the legacy values, engine pod membership
(``pod_dispatch``), locality pins to engines, the machine pause fields,
and the engine usage rule in dispatch and preflight.

Django-internal pins (``matcher.*`` direct calls, ``_pinned_runner_for``,
the 0035 data migration, ``execution_fields``) have no Rust counterpart —
they are covered by the Rust unit suites (matcher fixtures, admission,
policy) and are not re-pinned here. Every case below drives HTTP (web
app serializers, daemon sessions, pause/resume drains, the runner
enroll flow) and diffs Postgres through ``DATABASE_URL``.
"""

from __future__ import annotations

import pytest

from _harness.web import web_patch, web_post

from .conftest import DAEMON

pytestmark = pytest.mark.contract

WEB_RUNNERS = "/api/runners"


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------


def _issue_url(workspace, project, issue_id):
    return (
        f"/api/workspaces/{workspace['slug']}/projects/{project['id']}"
        f"/issues/{issue_id}/"
    )


def _mk_runner(seeder, owner_id, workspace_id, pod_id, *, name=None,
               provisioning="manual", status="online", pod_dispatch=None,
               dev_machine_id=None):
    """Seed a runner row; ``pod_dispatch=None`` leaves the DB default."""
    import uuid

    runner_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO runner
           (id, owner_id, workspace_id, pod_id, name, host_label,
            provisioning, visibility, status, capabilities,
            os, arch, runner_version,
            refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version,
            enrollment_token_hash, enrollment_token_fingerprint,
            enrolled_at, revoked_at, revoked_reason,
            dev_machine_id, dev_metadata, protocol_version,
            created_at, updated_at)
           VALUES (%s,%s,%s,%s,%s,'', 'manual',0,'offline','[]',
                   '','','',
                   '','',0,'',1,
                   '','',
                   NULL,NULL,'',
                   NULL,'{}',1,
                   now(),now())""",
        (
            runner_id,
            owner_id,
            workspace_id,
            pod_id,
            name or f"engine-{runner_id[:8]}",
        ),
    )
    seeder._put("runner", runner_id)
    if provisioning != "manual" or pod_dispatch is not None or dev_machine_id:
        seeder.db.execute(
            """UPDATE runner SET provisioning = %s, pod_dispatch = %s,
               dev_machine_id = %s WHERE id = %s""",
            (provisioning, pod_dispatch or "pod_member", dev_machine_id, runner_id),
        )
    _go_online_status(seeder, runner_id, status)
    return runner_id


def _go_online_status(seeder, runner_id, status):
    if status == "online":
        seeder.db.execute(
            "UPDATE runner SET status = 'online', last_heartbeat_at = now() WHERE id = %s",
            (runner_id,),
        )
    elif status == "offline":
        seeder.db.execute(
            "UPDATE runner SET status = 'offline', last_heartbeat_at = NULL WHERE id = %s",
            (runner_id,),
        )


def _go_online(seeder, runner_id):
    seeder.db.execute(
        "UPDATE runner SET status = 'online', last_heartbeat_at = now() WHERE id = %s",
        (runner_id,),
    )


def _open_session(machine_client, runner_id):
    response = machine_client.post(
        f"{DAEMON}/runners/{runner_id}/sessions/", json={}
    )
    assert response.status_code == 201, response.text


def _engine_client(settings, seeder, owner_id, workspace_id, machine_id, runner_id):
    """Bearer ``mt_`` client for a seeded engine (token bound to its machine)."""
    from _harness.http import bearer_client

    raw = f"mt_{seeder.tag}-{runner_id[:8]}".encode().decode()
    seeder.create_machine_token(
        owner_id, workspace_id, raw,
        host_label=f"eng-{seeder.tag}"[:200], dev_machine_id=machine_id,
    )
    return bearer_client(
        settings.base_url, raw, headers={"X-Runner-Id": runner_id}
    )


def _resume(user_client, runner_id):
    response = web_post(user_client, f"{WEB_RUNNERS}/{runner_id}/resume/", {})
    assert response.status_code == 200, response.text
    return response


def _pause(user_client, runner_id):
    response = web_post(user_client, f"{WEB_RUNNERS}/{runner_id}/pause/", {})
    assert response.status_code == 200, response.text
    return response


def _drain_now(user_client, runner_id):
    """Pause then resume: resume drains the pod; a bare resume is a no-op
    on an unpaused runner (Django ``RunnerResumeEndpoint``)."""
    _pause(user_client, runner_id)
    _resume(user_client, runner_id)


def _queued_run(seeder, workspace_id, owner_id, pod_id, issue_id, *,
                created_by, trigger="run_ai", executor_kind="local_runner",
                pinned_runner_id=None):
    import uuid

    run_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, workspace_id, owner_id, created_by_id, pod_id, work_item_id,
            runner_id, pinned_runner_id, status, prompt, trigger,
            executor_kind, run_config, required_capabilities, thread_id,
            llm_model, usage, dispatch_attempts, cancel_reason,
            error_details, tool_plan, phase_kind, agent_metadata, pin_kind,
            created_at)
           VALUES (%s,%s,%s,%s,%s,%s,
                   NULL,%s,'queued','',%s,
                   %s,'{}','[]','','','{}',0,'',
                   '{}','{}','','{}','locality',
                   now())""",
        (
            run_id, workspace_id, owner_id, created_by, pod_id, issue_id,
            pinned_runner_id, trigger, executor_kind,
        ),
    )
    seeder._put("agent_run", run_id)
    return run_id


def _run_row(seeder, run_id):
    return seeder.db.fetchone(
        "SELECT status, runner_id, pinned_runner_id FROM agent_run WHERE id = %s",
        (run_id,),
    )


def _issue_row(seeder, issue_id):
    return seeder.db.fetchone(
        "SELECT agent_executor, assigned_pod_id FROM issues WHERE id = %s",
        (issue_id,),
    )


def _project_row(seeder, project_id):
    return seeder.db.fetchone(
        "SELECT default_agent_executor FROM projects WHERE id = %s",
        (project_id,),
    )


def _engine_world(seeder, daemon_world, *, pod_dispatch="pod_member"):
    """Engine runner + dev machine in the daemon world, online, with a
    live session row (drain frame delivery needs it)."""
    import uuid

    owner_id = daemon_world["owner"]["id"]
    workspace = daemon_world["workspace"]
    pod = daemon_world["pod"]
    machine = seeder.create_dev_machine(owner_id)
    engine_id = _mk_runner(
        seeder, owner_id, workspace["id"], pod["id"],
        provisioning="desktop_bundled", pod_dispatch=pod_dispatch,
        dev_machine_id=machine["id"],
    )
    _go_online(seeder, engine_id)
    seeder.db.execute(
        "INSERT INTO runner_session (id, runner_id, protocol_version,"
        " created_at, last_seen_at, revoked_at, revoked_reason)"
        " VALUES (%s,%s,4,now(),now(),NULL,'')",
        (str(uuid.uuid4()), engine_id),
    )
    return engine_id, machine["id"]


def _issue_in_backlog(seeder, daemon_world, *, created_by=None):
    project = daemon_world["project"]
    seeder.create_project_member(
        daemon_world["workspace"]["id"], project["id"],
        daemon_world["owner"]["id"],
    )
    state = seeder.create_state(
        daemon_world["workspace"]["id"],
        project["id"],
    )
    issue = seeder.create_issue(
        daemon_world["workspace"]["id"], project["id"], state["id"],
    )
    # ``BaseModel.save`` blanks created_by outside a request; the usage
    # rule reads it, so write it directly (Django test fixture parity).
    creator = created_by or daemon_world["owner"]["id"]
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s, updated_by_id = %s WHERE id = %s",
        (creator, creator, issue["id"]),
    )
    return issue["id"]


# ---------------------------------------------------------------------------
# §6 Executor model: options, project/issue write translation
# ---------------------------------------------------------------------------


def test_executor_options_return_deprecated_entries(user_client, daemon_world, seeder):
    project = daemon_world["project"]
    seeder.create_project_member(
        daemon_world["workspace"]["id"], project["id"],
        daemon_world["owner"]["id"],
    )
    response = user_client.get(
        f"/api/workspaces/{daemon_world['workspace']['slug']}/projects/{project['id']}/"
    )
    assert response.status_code == 200, response.text
    rows = response.json()["agent_executor_options"]
    assert [row["kind"] for row in rows] == [
        "cloud_agent", "local_runner", "managed_runner",
    ]
    options = {row["kind"]: row for row in rows}
    assert options["local_runner"]["deprecated"] is False
    assert options["cloud_agent"]["deprecated"] is True
    assert options["managed_runner"]["deprecated"] is True


@pytest.mark.parametrize("legacy", ["cloud_agent", "managed_runner"])
def test_project_default_executor_write_is_translated(
    user_client, daemon_world, seeder, legacy
):
    project = daemon_world["project"]
    seeder.create_project_member(
        daemon_world["workspace"]["id"], project["id"],
        daemon_world["owner"]["id"],
    )
    url = (
        f"/api/workspaces/{daemon_world['workspace']['slug']}"
        f"/projects/{project['id']}/"
    )
    response = web_patch(
        user_client, url, {"default_agent_executor": legacy}
    )
    assert response.status_code == 200, response.text
    row = _project_row(seeder, project["id"])
    assert row["default_agent_executor"] == "local_runner"


def test_issue_cloud_agent_write_becomes_pod_level(
    user_client, daemon_world, seeder
):
    issue_id = _issue_in_backlog(seeder, daemon_world)
    url = _issue_url(daemon_world["workspace"], daemon_world["project"], issue_id)
    response = web_patch(user_client, url, {"agent_executor": "cloud_agent"})
    assert response.status_code in (200, 204), response.text
    row = _issue_row(seeder, issue_id)
    assert row["agent_executor"] == "local_runner"


def test_issue_managed_write_keeps_assigned_pod(user_client, daemon_world, seeder):
    project = daemon_world["project"]
    other_pod = seeder.create_pod(
        daemon_world["workspace"]["id"], project["id"],
        name=f"Other-{seeder.tag}"[:100],
    )
    issue_id = _issue_in_backlog(seeder, daemon_world)
    seeder.db.execute(
        "UPDATE issues SET assigned_pod_id = %s WHERE id = %s",
        (other_pod["id"], issue_id),
    )
    url = _issue_url(daemon_world["workspace"], project, issue_id)
    response = web_patch(user_client, url, {"agent_executor": "managed_runner"})
    assert response.status_code in (200, 204), response.text
    row = _issue_row(seeder, issue_id)
    assert row["agent_executor"] == "local_runner"
    assert str(row["assigned_pod_id"]) == other_pod["id"]


def test_issue_executor_write_refused_under_a_live_run(
    user_client, daemon_world, seeder
):
    ws = daemon_world["workspace"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    seeder.db.execute(
        "UPDATE issues SET agent_executor = 'cloud_agent' WHERE id = %s",
        (issue_id,),
    )
    _queued_run(
        seeder, ws["id"], daemon_world["owner"]["id"], daemon_world["pod"]["id"],
        issue_id, created_by=daemon_world["owner"]["id"],
    )
    url = _issue_url(ws, daemon_world["project"], issue_id)
    response = web_patch(user_client, url, {"agent_executor": "local_runner"})
    assert response.status_code == 400, response.text
    assert "agent_executor" in response.json()


# ---------------------------------------------------------------------------
# §7 Pod membership: pinned-only vs pod-member matching, legacy pins
# ---------------------------------------------------------------------------


def test_pinned_only_engine_takes_no_pod_work_unlocked_by_patch(
    user_client, settings, seeder, daemon_world
):
    """A ``pinned_only`` engine drains nothing; opening pod work via the
    runner PATCH lets the same run assign on the next resume drain."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
    )
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pinned_only"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)

    _resume(user_client, engine_id)
    assert _run_row(seeder, run_id)["status"] == "queued"

    response = web_patch(
        user_client, f"{WEB_RUNNERS}/{engine_id}/",
        {"pod_dispatch": "pod_member"},
    )
    assert response.status_code == 200, response.text
    _resume(user_client, engine_id)
    row = _run_row(seeder, run_id)
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) == engine_id


def test_legacy_managed_run_is_served_only_to_its_engine(
    user_client, settings, seeder, daemon_world
):
    """A legacy ``managed_runner`` run reaches its engine even while
    ``pinned_only``; an own-agent runner in the pod never takes it."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pinned_only"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)
    legacy_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
        executor_kind="managed_runner", pinned_runner_id=engine_id,
    )

    _drain_now(user_client, engine_id)
    row = _run_row(seeder, legacy_id)
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) == engine_id


def test_own_agent_runner_never_takes_unpinned_legacy_managed_run(
    user_client, machine_client, seeder, daemon_world, machine_flow
):
    """An unpinned legacy run is dead work: no runner takes it."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    legacy_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
        executor_kind="managed_runner",
    )
    _open_session(machine_client, machine_flow["runner_id"])
    _go_online(seeder, machine_flow["runner_id"])

    _resume(user_client, machine_flow["runner_id"])
    assert _run_row(seeder, legacy_id)["status"] == "queued"


def test_pod_dispatch_patch_releases_pins_on_pinned_only(
    user_client, seeder, daemon_world
):
    """Turning pod work off returns locality pins to the pod."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    engine_id, _ = _engine_world(seeder, daemon_world, pod_dispatch="pod_member")
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
        pinned_runner_id=engine_id,
    )

    response = web_patch(
        user_client, f"{WEB_RUNNERS}/{engine_id}/",
        {"pod_dispatch": "pinned_only"},
    )
    assert response.status_code == 200, response.text
    row = _run_row(seeder, run_id)
    assert row["pinned_runner_id"] is None
    assert row["status"] == "queued"


# ---------------------------------------------------------------------------
# §8 Usage rule: pause blocks matching, teammate admission, auto runs
# ---------------------------------------------------------------------------


def test_owner_pause_blocks_engine_matching(
    user_client, settings, seeder, daemon_world
):
    """Seeding the pause column directly: the engine drains nothing until
    the machine reports the owner pause cleared."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
    )
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pod_member"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)

    seeder.db.execute(
        "UPDATE dev_machine SET engine_paused_by_owner = true WHERE id = %s",
        (machine_id,),
    )

    _drain_now(user_client, engine_id)
    assert _run_row(seeder, run_id)["status"] == "queued"

    seeder.db.execute(
        "UPDATE dev_machine SET engine_paused_by_owner = false WHERE id = %s",
        (machine_id,),
    )
    _drain_now(user_client, engine_id)
    row = _run_row(seeder, run_id)
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) == engine_id


def test_auto_pause_blocks_engine_matching(
    user_client, settings, seeder, daemon_world
):
    """``engine_auto_pause_reason`` counts as paused for matching."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id, created_by=owner_id,
    )
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pod_member"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)
    seeder.db.execute(
        "UPDATE dev_machine SET engine_auto_pause_reason = 'wallet_empty' WHERE id = %s",
        (machine_id,),
    )

    _drain_now(user_client, engine_id)
    assert _run_row(seeder, run_id)["status"] == "queued"


def _teammate_world(seeder, daemon_world):
    teammate = seeder.create_user(email=f"mate-{seeder.tag}@example.com")
    seeder.create_workspace_member(daemon_world["workspace"]["id"], teammate["id"])
    seeder.create_project_member(
        daemon_world["workspace"]["id"], daemon_world["project"]["id"],
        teammate["id"],
    )
    return teammate


def test_teammate_click_not_admitted_to_engine_without_opt_in(
    user_client, settings, seeder, daemon_world
):
    """Scenario 7: a teammate's human run on the owner's issue waits in the
    pod while ``engine_team_runs`` is false; the opt-in admits it."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    teammate = _teammate_world(seeder, daemon_world)
    issue_id = _issue_in_backlog(seeder, daemon_world, created_by=owner_id)
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id,
        created_by=teammate["id"], trigger="run_ai",
    )
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pod_member"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)

    _drain_now(user_client, engine_id)
    assert _run_row(seeder, run_id)["status"] == "queued"

    seeder.db.execute(
        "UPDATE dev_machine SET engine_team_runs = true WHERE id = %s",
        (machine_id,),
    )
    _drain_now(user_client, engine_id)
    row = _run_row(seeder, run_id)
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) == engine_id


def test_automatic_run_on_owners_issue_is_admitted_to_engine(
    user_client, settings, seeder, daemon_world
):
    """A tick run on the owner's issue is admitted to a pod-member engine."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world, created_by=owner_id)
    run_id = _queued_run(
        seeder, ws["id"], owner_id, pod["id"], issue_id,
        created_by=owner_id, trigger="tick",
    )
    engine_id, machine_id = _engine_world(
        seeder, daemon_world, pod_dispatch="pod_member"
    )
    with _engine_client(
        settings, seeder, owner_id, ws["id"], machine_id, engine_id
    ) as engine_client:
        _open_session(engine_client, engine_id)

    _drain_now(user_client, engine_id)
    row = _run_row(seeder, run_id)
    assert row["status"] == "assigned"
    assert str(row["runner_id"]) == engine_id


# ---------------------------------------------------------------------------
# Preflight: the tick counts pod-member engines only; offline counts
# ---------------------------------------------------------------------------


def _v1_client(seeder, daemon_world):
    """X-Api-Key client for the world owner (v1 work items)."""
    from _harness.http import api_key_client
    from _harness.config import get_settings

    settings = get_settings()
    client = api_key_client(settings.base_url, daemon_world["api_token"]["token"])
    return client


def _project_detail_url(workspace, project_id, issue_id):
    return (
        f"/api/v1/workspaces/{workspace['slug']}/projects/{project_id}"
        f"/work-items/{issue_id}/"
    )


def test_tick_preflight_counts_pod_member_engine_only(
    seeder, daemon_world, user_client
):
    """Backlog → In Progress bounces with only a ``pinned_only`` engine in
    the pod, and dispatches once the engine takes pod work."""
    ws = daemon_world["workspace"]
    project = daemon_world["project"]
    seeder.create_project_member(
        ws["id"], project["id"], daemon_world["owner"]["id"]
    )
    backlog = seeder.create_state(ws["id"], project["id"], name="Backlog")
    started = seeder.create_state(
        ws["id"], project["id"], name="In Progress", group="started",
    )
    engine_id, _ = _engine_world(seeder, daemon_world, pod_dispatch="pinned_only")

    seeder.db.execute("UPDATE projects SET default_state_id = %s WHERE id = %s", (backlog["id"], project["id"]))
    owner_id = daemon_world["owner"]["id"]
    issue_id = seeder.create_issue(ws["id"], project["id"], backlog["id"])["id"]
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s, updated_by_id = %s WHERE id = %s",
        (owner_id, owner_id, issue_id),
    )
    client = _v1_client(seeder, daemon_world)

    def _move():
        return client.patch(
            _project_detail_url(ws, project["id"], issue_id),
            json={"state": started["id"]},
        )

    response = _move()
    assert response.status_code == 200, response.text
    # Bounced back to Backlog.
    row = seeder.db.fetchone(
        "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert str(row["state_id"]) == backlog["id"]

    seeder.db.execute(
        "UPDATE runner SET pod_dispatch = 'pod_member' WHERE id = %s",
        (engine_id,),
    )
    # Second issue so the tick is fresh.
    issue2 = seeder.create_issue(ws["id"], project["id"], backlog["id"])["id"]
    seeder.db.execute(
        "UPDATE issues SET created_by_id = %s, updated_by_id = %s WHERE id = %s",
        (owner_id, owner_id, issue2),
    )
    response = client.patch(
        _project_detail_url(ws, project["id"], issue2),
        json={"state": started["id"]},
    )
    assert response.status_code == 200, response.text
    row = seeder.db.fetchone(
        "SELECT state_id FROM issues WHERE id = %s", (issue2,)
    )
    assert str(row["state_id"]) == started["id"]


def test_preflight_counts_offline_pod_member_engine(seeder, daemon_world):
    """An offline pod-member engine still counts: the run waits rather
    than the issue bouncing."""
    ws = daemon_world["workspace"]
    project = daemon_world["project"]
    seeder.create_project_member(
        ws["id"], project["id"], daemon_world["owner"]["id"]
    )
    backlog = seeder.create_state(ws["id"], project["id"], name="Backlog")
    started = seeder.create_state(
        ws["id"], project["id"], name="In Progress", group="started",
    )
    engine_id, _ = _engine_world(seeder, daemon_world, pod_dispatch="pod_member")
    seeder.db.execute(
        "UPDATE runner SET status = 'offline', last_heartbeat_at = NULL WHERE id = %s",
        (engine_id,),
    )
    seeder.db.execute("UPDATE projects SET default_state_id = %s WHERE id = %s", (backlog["id"], project["id"]))
    issue_id = seeder.create_issue(ws["id"], project["id"], backlog["id"])["id"]
    client = _v1_client(seeder, daemon_world)
    response = client.patch(
        _project_detail_url(ws, project["id"], issue_id),
        json={"state": started["id"]},
    )
    assert response.status_code == 200, response.text
    row = seeder.db.fetchone(
        "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert str(row["state_id"]) == started["id"]


# ---------------------------------------------------------------------------
# New engine runner defaults to pinned_only (runner enroll flow)
# ---------------------------------------------------------------------------


def test_new_engine_runner_defaults_to_pinned_only(settings, seeder, daemon_world):
    """``POST /api/v1/runner/runners/`` on a machine whose provisioning is
    ``desktop_bundled`` mints an engine runner that takes no pod work."""
    from _harness.http import api_key_client

    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    machine = seeder.create_dev_machine(owner_id)
    seeder.db.execute(
        "UPDATE dev_machine SET provisioning = 'desktop_bundled' WHERE id = %s",
        (machine["id"],),
    )
    # The endpoint resolves the machine by host_label; the seeded legacy
    # desktop machine must be the one it finds.
    host_label = machine["host_label"]
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": ws["slug"],
                "host_label": host_label,
                "agent_kind": "engine",
            },
        )
    assert response.status_code == 201, response.text
    runner_id = response.json()["runner_id"]
    row = seeder.db.fetchone(
        "SELECT provisioning, pod_dispatch FROM runner WHERE id = %s",
        (runner_id,),
    )
    assert row["provisioning"] == "desktop_bundled"
    # The created row must take no pod work: it can only be an engine
    # created through the daemon flow, and the server defaults those to
    # pinned_only.
    assert row["pod_dispatch"] == "pinned_only"
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (owner_id, host_label),
    )
    seeder.db.execute("DELETE FROM runner WHERE id = %s", (runner_id,))
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (owner_id, host_label),
    )
