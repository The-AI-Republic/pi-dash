# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dispatch core of "the built-in engine as a first-class runner" (PIDESKAPP-123).

Contract tests for ``.ai_design/built_engine_as_runner/design.md`` §6-§9 and
the server data migration of §13: the single Runner executor, strict pins from
``Issue.assigned_runner`` and every path that must not release them, engine
pod membership, the engine usage rule, and the 12 h expiry of waiting runs.
Web-app endpoints are exercised with ``session_client`` (``pi_dash.app`` /
``/api/``).
"""

from __future__ import annotations

import importlib
from datetime import timedelta
from unittest.mock import patch

import pytest
from django.apps import apps as django_apps
from django.db import transaction
from django.test import override_settings
from django.urls import reverse
from django.utils import timezone

from pi_dash.cloud_agent.creation import execution_fields
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.db.models import Issue, IssueActivity, Project, ProjectMember, State, User, WorkspaceMember
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    AgentRunTrigger,
    DevMachine,
    Pod,
    Runner,
    RunnerPodDispatch,
    RunnerProvisioning,
    RunnerStatus,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.services.dispatch_rules import (
    ASSIGNED_RUNNER_NOT_OWNER,
    ASSIGNED_RUNNER_OFFLINE,
    ASSIGNED_RUNNER_REMOVED,
    WAITING_FOR_RUNNER,
    AssignedRunnerUnavailable,
)

pytestmark = pytest.mark.contract


# --------------------------------------------------------------------------
# Fixtures
# --------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _no_outbound(monkeypatch):
    """Keep runner frames and issue background tasks in-process."""
    monkeypatch.setattr("pi_dash.runner.services.pubsub.send_to_runner", lambda *a, **k: None)
    from pi_dash.app.views.issue import base

    for task in (
        base.issue_activity,
        base.model_activity,
        base.issue_description_version_task,
        base.recent_visited_task,
    ):
        monkeypatch.setattr(task, "delay", lambda *a, **k: None)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def other_pod(project, create_user):
    return Pod.objects.create(
        workspace=project.workspace, project=project, name=f"{project.identifier}_b", created_by=create_user
    )


@pytest.fixture
def teammate(workspace, project):
    user = User.objects.create(email="teammate@example.com", username="teammate", first_name="Team")
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=15)
    ProjectMember.objects.create(workspace=workspace, project=project, member=user, role=15)
    return user


@pytest.fixture
def member(workspace, project, create_user):
    ProjectMember.objects.get_or_create(
        workspace=workspace, project=project, member=create_user, defaults={"role": 20}
    )
    return create_user


@pytest.fixture
def machine(create_user):
    return DevMachine.objects.create(
        owner=create_user, host_label="rich-mbp", label="Rich's MacBook", last_seen_at=timezone.now()
    )


def _runner(owner, pod, *, name, engine=False, machine=None, status=RunnerStatus.ONLINE, **extra):
    return Runner.objects.create(
        owner=owner,
        workspace=pod.workspace,
        pod=pod,
        dev_machine=machine,
        name=name,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED if engine else RunnerProvisioning.MANUAL,
        status=status,
        last_heartbeat_at=timezone.now() if status in (RunnerStatus.ONLINE, RunnerStatus.BUSY) else None,
        enrolled_at=timezone.now(),
        **extra,
    )


@pytest.fixture
def engine(create_user, pod, machine):
    return _runner(create_user, pod, name="builtin-rich", engine=True, machine=machine)


@pytest.fixture
def issue(project, create_user, member):
    state = State.objects.create(name="Todo", group="unstarted", project=project)
    created = Issue.objects.create(workspace=project.workspace, project=project, state=state, name="Task")
    # ``BaseModel.save`` blanks ``created_by`` outside a request; the usage
    # rule reads it, so write it directly.
    Issue.objects.filter(pk=created.pk).update(created_by=create_user)
    created.refresh_from_db()
    return created


def _assign(issue, runner):
    Issue.objects.filter(pk=issue.pk).update(assigned_runner=runner, assigned_pod=runner.pod)
    issue.refresh_from_db()
    return issue


def _queued(issue, *, created_by, pinned_runner=None, strict=False, trigger=AgentRunTrigger.RUN_AI, pod=None):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=created_by,
        pod=pod or (pinned_runner.pod if pinned_runner else issue.assigned_pod),
        work_item=issue,
        pinned_runner=pinned_runner,
        pin_strict=strict,
        trigger=trigger,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )


def _issue_url(workspace, project, issue):
    return f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/{issue.id}/"


# --------------------------------------------------------------------------
# §6 Executor model
# --------------------------------------------------------------------------


def test_executor_options_return_deprecated_entries(session_client, workspace, project, member, monkeypatch):
    from pi_dash.app.views.project import base

    monkeypatch.setattr(base.recent_visited_task, "delay", lambda **kwargs: None)
    resp = session_client.get(f"/api/workspaces/{workspace.slug}/projects/{project.id}/")
    assert resp.status_code == 200, resp.content
    options = {opt["kind"]: opt for opt in resp.json()["agent_executor_options"]}
    assert set(options) == {"local_runner", "cloud_agent", "managed_runner"}
    assert options["local_runner"]["deprecated"] is False
    assert options["cloud_agent"]["deprecated"] is True
    assert options["managed_runner"]["deprecated"] is True


@pytest.mark.parametrize("legacy", ["cloud_agent", "managed_runner"])
def test_project_default_executor_write_is_translated(session_client, workspace, project, member, monkeypatch, legacy):
    from pi_dash.app.views.project import base

    monkeypatch.setattr(base.model_activity, "delay", lambda **kwargs: None)
    resp = session_client.patch(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}/",
        {"default_agent_executor": legacy},
        format="json",
    )
    assert resp.status_code == 200, resp.content
    project.refresh_from_db()
    assert project.default_agent_executor == AgentExecutorKind.LOCAL_RUNNER


def test_issue_cloud_agent_write_becomes_pod_level_runner(session_client, workspace, project, issue):
    resp = session_client.patch(_issue_url(workspace, project, issue), {"agent_executor": "cloud_agent"}, format="json")
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.agent_executor == AgentExecutorKind.LOCAL_RUNNER
    assert issue.assigned_runner_id is None


@pytest.mark.parametrize("online", [True, False])
def test_issue_managed_write_assigns_callers_engine(session_client, workspace, project, issue, engine, online):
    if not online:
        Runner.objects.filter(pk=engine.pk).update(status=RunnerStatus.OFFLINE, last_heartbeat_at=None)
    resp = session_client.patch(
        _issue_url(workspace, project, issue), {"agent_executor": "managed_runner"}, format="json"
    )
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.agent_executor == AgentExecutorKind.LOCAL_RUNNER
    assert issue.assigned_runner_id == engine.id
    assert issue.assigned_pod_id == engine.pod_id


@override_settings(CLOUD_AGENT_ENABLED=True)
def test_queued_cloud_agent_runs_are_still_readable(project, create_user):
    """The Cloud Agent pipeline stays: existing rows keep their kind and the
    check constraint still holds them."""
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=Pod.default_for_project(project),
        executor_kind=AgentExecutorKind.CLOUD_AGENT,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )
    run.refresh_from_db()
    assert run.executor_kind == AgentExecutorKind.CLOUD_AGENT


# --------------------------------------------------------------------------
# §7 Assigned runner and the dispatch-pod invariant
# --------------------------------------------------------------------------


def test_setting_assigned_runner_sets_pod_and_choosing_pod_clears_it(
    session_client, workspace, project, issue, other_pod, create_user
):
    runner = _runner(create_user, other_pod, name="mini")
    url = _issue_url(workspace, project, issue)
    resp = session_client.patch(url, {"assigned_runner_id": str(runner.id)}, format="json")
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner.id
    assert issue.assigned_pod_id == other_pod.id

    # Re-sending the current pod (the web form re-PATCHes on blur) is a no-op.
    resp = session_client.patch(url, {"assigned_pod_id": str(other_pod.id)}, format="json")
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner.id

    default_pod = Pod.default_for_project(project)
    resp = session_client.patch(url, {"assigned_pod_id": str(default_pod.id)}, format="json")
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None
    assert issue.assigned_pod_id == default_pod.id


def test_only_the_owner_can_assign_an_issue_to_a_runner(session_client, workspace, project, issue, pod, teammate):
    theirs = _runner(teammate, pod, name="their-engine", engine=True)
    resp = session_client.patch(
        _issue_url(workspace, project, issue), {"assigned_runner_id": str(theirs.id)}, format="json"
    )
    assert resp.status_code == 400, resp.content
    assert "assigned_runner_id" in resp.json()


def test_reassignment_repins_queued_runs(session_client, workspace, project, issue, engine, other_pod, create_user):
    _assign(issue, engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    mini = _runner(create_user, other_pod, name="mini")
    resp = session_client.patch(
        _issue_url(workspace, project, issue), {"assigned_runner_id": str(mini.id)}, format="json"
    )
    assert resp.status_code in (200, 204), resp.content
    run.refresh_from_db()
    assert run.pinned_runner_id == mini.id
    assert run.pin_strict is True
    assert run.pod_id == other_pod.id


def test_pod_move_rehomes_assigned_issues_and_strict_runs(session_client, issue, engine, other_pod, create_user):
    _assign(issue, engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    resp = session_client.patch(
        reverse("runner-detail", kwargs={"runner_id": engine.id}), {"pod": str(other_pod.id)}, format="json"
    )
    assert resp.status_code == 200, resp.content
    issue.refresh_from_db()
    run.refresh_from_db()
    assert issue.assigned_pod_id == other_pod.id
    assert run.pod_id == other_pod.id
    # The next dispatch happens in the new pod.
    engine.refresh_from_db()
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


def test_cross_project_move_refused_while_issues_are_assigned(
    session_client, workspace, issue, pod, create_user
):
    other_project = Project.objects.create(name="Other", identifier="OTH", workspace=workspace)
    target = Pod.default_for_project(other_project)
    runner = _runner(create_user, pod, name="mini")
    _assign(issue, runner)
    url = reverse("runner-detail", kwargs={"runner_id": runner.id})
    resp = session_client.patch(url, {"pod": str(target.id)}, format="json")
    assert resp.status_code == 409, resp.content
    body = resp.json()
    assert body["code"] == "runner_has_assigned_issues"
    assert [row["id"] for row in body["issues"]] == [str(issue.id)]


def test_engine_moves_only_within_its_project(session_client, workspace, engine):
    other_project = Project.objects.create(name="Other", identifier="OTH", workspace=workspace)
    resp = session_client.patch(
        reverse("runner-detail", kwargs={"runner_id": engine.id}),
        {"pod": str(Pod.default_for_project(other_project).id)},
        format="json",
    )
    assert resp.status_code == 400
    assert resp.json()["code"] == "engine_runner_project_fixed"


# --------------------------------------------------------------------------
# §7 Strict pins are never released
# --------------------------------------------------------------------------


def test_strict_pin_survives_busy_rejection(issue, engine, pod, create_user):
    from pi_dash.runner.services.run_lifecycle import apply_assign_rejected_busy

    _assign(issue, engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    AgentRun.objects.filter(pk=run.pk).update(
        runner=engine, status=AgentRunStatus.ASSIGNED, assigned_at=timezone.now()
    )
    bystander = _runner(create_user, pod, name="idle-mini")
    with transaction.atomic():
        apply_assign_rejected_busy(engine, run.id)
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None
    assert run.pinned_runner_id == engine.id
    # The idle own-agent runner in the same pod never takes it.
    matcher.drain_pod(pod)
    run.refresh_from_db()
    assert run.runner_id is None
    with transaction.atomic():
        assert matcher.next_for_runner(bystander) is None


def test_non_strict_pin_is_still_released_on_busy_rejection(issue, pod, create_user):
    from pi_dash.runner.services.run_lifecycle import apply_assign_rejected_busy

    mini = _runner(create_user, pod, name="mini")
    run = _queued(issue, created_by=create_user, pinned_runner=mini, strict=False)
    AgentRun.objects.filter(pk=run.pk).update(runner=mini, status=AgentRunStatus.ASSIGNED)
    with transaction.atomic():
        apply_assign_rejected_busy(mini, run.id)
    run.refresh_from_db()
    assert run.pinned_runner_id is None


def test_release_pin_endpoint_refuses_a_strict_pin(session_client, issue, engine, create_user):
    _assign(issue, engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    resp = session_client.post(reverse("runner-run-release-pin", kwargs={"run_id": run.id}))
    assert resp.status_code == 409, resp.content
    assert resp.json()["code"] == "pin_strict"
    run.refresh_from_db()
    assert run.pinned_runner_id == engine.id


def test_offline_sweep_skips_strict_pins(issue, engine, create_user):
    from pi_dash.runner.tasks import release_pins_on_offline_runners

    _assign(issue, engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    Runner.objects.filter(pk=engine.pk).update(status=RunnerStatus.OFFLINE, last_heartbeat_at=None)
    AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - timedelta(hours=1))
    assert release_pins_on_offline_runners() == 0
    run.refresh_from_db()
    assert run.pinned_runner_id == engine.id


def test_revoke_fails_strict_runs_and_makes_issues_pod_level(session_client, issue, engine, create_user):
    _assign(issue, engine)
    pod_before = issue.assigned_pod_id
    strict = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    resp = session_client.post(reverse("runner-revoke", kwargs={"runner_id": engine.id}))
    assert resp.status_code == 200, resp.content
    strict.refresh_from_db()
    issue.refresh_from_db()
    assert strict.status == AgentRunStatus.FAILED
    assert strict.error_code == ASSIGNED_RUNNER_REMOVED
    assert issue.assigned_runner_id is None
    assert issue.assigned_pod_id == pod_before
    assert IssueActivity.objects.filter(issue=issue, field="assigned_runner").exists()


def test_hard_delete_never_turns_a_strict_run_into_a_pod_run(issue, engine, create_user):
    _assign(issue, engine)
    strict = _queued(issue, created_by=create_user, pinned_runner=engine, strict=True)
    Runner.objects.filter(pk=engine.pk).delete()
    strict.refresh_from_db()
    issue.refresh_from_db()
    assert strict.status == AgentRunStatus.FAILED
    assert strict.error_code == ASSIGNED_RUNNER_REMOVED
    assert issue.assigned_runner_id is None


# --------------------------------------------------------------------------
# §7 Non-strict pins and engine pod membership
# --------------------------------------------------------------------------


def test_continuation_pins_to_pod_member_engine_and_is_released_when_pod_work_off(
    session_client, issue, engine, pod, create_user
):
    from pi_dash.orchestration.service import _pinned_runner_for

    parent = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        runner=engine,
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    # pinned_only: a continuation is created unpinned in the pod.
    assert _pinned_runner_for(parent, pod) is None

    url = reverse("runner-detail", kwargs={"runner_id": engine.id})
    resp = session_client.patch(url, {"pod_dispatch": "pod_member"}, format="json")
    assert resp.status_code == 200, resp.content
    assert resp.json()["pod_dispatch"] == "pod_member"
    engine.refresh_from_db()
    assert _pinned_runner_for(parent, pod) == engine

    continuation = _queued(issue, created_by=create_user, pinned_runner=engine, strict=False, pod=pod)
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == continuation.id

    resp = session_client.patch(url, {"pod_dispatch": "pinned_only"}, format="json")
    assert resp.status_code == 200, resp.content
    continuation.refresh_from_db()
    assert continuation.pinned_runner_id is None


def test_owner_pause_releases_non_strict_pins_and_blocks_matching(
    session_client, workspace, issue, engine, machine, pod, create_user
):
    Runner.objects.filter(pk=engine.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    nonstrict = _queued(issue, created_by=create_user, pinned_runner=engine, strict=False, pod=pod)
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = session_client.patch(url, {"workspace": str(workspace.id), "engine_paused_by_owner": True}, format="json")
    assert resp.status_code == 200, resp.content
    assert resp.json()["engine_paused_by_owner"] is True
    nonstrict.refresh_from_db()
    assert nonstrict.pinned_runner_id is None
    assert not Runner.objects.assignable().filter(pk=engine.pk).exists()

    resp = session_client.patch(url, {"workspace": str(workspace.id), "engine_paused_by_owner": False}, format="json")
    assert resp.status_code == 200
    assert Runner.objects.assignable().filter(pk=engine.pk).exists()


def test_auto_pause_reason_blocks_matching(engine, machine):
    from pi_dash.runner.services.dispatch_rules import set_engine_auto_pause_reason

    assert set_engine_auto_pause_reason(machine, "wallet_empty") is True
    assert not Runner.objects.assignable().filter(pk=engine.pk).exists()
    assert set_engine_auto_pause_reason(machine, "") is True
    assert Runner.objects.assignable().filter(pk=engine.pk).exists()


def test_machine_patch_is_owner_only_and_engine_fields_only(api_client, workspace, machine, teammate):
    api_client.force_authenticate(user=teammate)
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = api_client.patch(url, {"workspace": str(workspace.id), "engine_team_runs": True}, format="json")
    assert resp.status_code == 404
    machine.refresh_from_db()
    assert machine.engine_team_runs is False


def test_machine_patch_rejects_server_set_fields(session_client, workspace, engine, machine):
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = session_client.patch(
        url, {"workspace": str(workspace.id), "engine_auto_pause_reason": ""}, format="json"
    )
    assert resp.status_code == 400


# --------------------------------------------------------------------------
# §8 Usage rule
# --------------------------------------------------------------------------


def test_teammate_click_not_admitted_to_engine_without_opt_in(issue, engine, machine, pod, teammate, create_user):
    Runner.objects.filter(pk=engine.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    engine.refresh_from_db()
    run = _queued(issue, created_by=teammate, trigger=AgentRunTrigger.RUN_AI, pod=pod)
    with transaction.atomic():
        assert matcher.next_for_runner(engine) is None
    # The same click on an own-agent runner of the owner is admitted (the
    # ordinary rule: the issue is the owner's).
    mini = _runner(create_user, pod, name="mini")
    with transaction.atomic():
        assert matcher.next_for_runner(mini).id == run.id

    machine.engine_team_runs = True
    machine.save(update_fields=["engine_team_runs"])
    engine.refresh_from_db()
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


def test_automatic_run_on_owners_issue_is_admitted_to_engine(issue, engine, pod, create_bot_user):
    Runner.objects.filter(pk=engine.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    engine.refresh_from_db()
    run = _queued(issue, created_by=create_bot_user, trigger=AgentRunTrigger.TICK, pod=pod)
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


def test_teammate_click_is_refused_at_creation_for_assigned_engine(issue, engine, teammate):
    _assign(issue, engine)
    with pytest.raises(AssignedRunnerUnavailable) as exc:
        execution_fields(project=issue.project, run_kind="issue", has_issue=True, actor=teammate, issue=issue)
    assert exc.value.code == ASSIGNED_RUNNER_NOT_OWNER


def test_preflight_bounce_does_not_count_engine_for_teammate_click(issue, engine, pod, teammate):
    Runner.objects.filter(pk=engine.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, teammate.id, human=True) is False
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is True


def test_tick_on_assigned_pinned_only_engine_is_allowed(issue, engine, pod, create_user):
    from pi_dash.orchestration import scheduling

    _assign(issue, engine)
    assert engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    creator = scheduling._resolve_creator_for_trigger(issue, triggered_by=scheduling.TRIGGER_TICK)
    assert creator == create_user
    assert scheduling.preflight_eligibility_or_bounce(
        issue, run_creator=creator, pod=pod, triggered_by=scheduling.TRIGGER_TICK
    )
    fields = execution_fields(
        project=issue.project, run_kind="issue", has_issue=True, actor=creator, automatic=True, issue=issue
    )
    assert fields["pinned_runner"] == engine and fields["pin_strict"] is True
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=creator,
        pod=pod,
        work_item=issue,
        trigger=AgentRunTrigger.TICK,
        status=AgentRunStatus.QUEUED,
        prompt="x",
        **fields,
    )
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


# --------------------------------------------------------------------------
# §9 Availability and the waiting queue
# --------------------------------------------------------------------------


def test_run_ai_on_offline_assigned_runner_is_refused(session_client, issue, engine):
    _assign(issue, engine)
    Runner.objects.filter(pk=engine.pk).update(status=RunnerStatus.OFFLINE, last_heartbeat_at=None)
    resp = session_client.post(
        reverse("runner-runs"), {"triggered_by": "run_ai", "work_item": str(issue.id)}, format="json"
    )
    assert resp.status_code == 409, resp.content
    body = resp.json()
    assert body["code"] == ASSIGNED_RUNNER_OFFLINE
    assert body["error"] == "Rich's MacBook is offline"
    assert not AgentRun.objects.filter(work_item=issue).exists()


def test_automatic_run_waits_for_offline_runner(issue, engine, create_user):
    _assign(issue, engine)
    Runner.objects.filter(pk=engine.pk).update(status=RunnerStatus.OFFLINE, last_heartbeat_at=None)
    fields = execution_fields(
        project=issue.project, run_kind="issue", has_issue=True, actor=create_user, automatic=True, issue=issue
    )
    assert fields["error_details"] == {"code": WAITING_FOR_RUNNER}


@override_settings(MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=3600)
def test_expiry_sweep_selects_by_pin_strict(issue, pod, create_user):
    """An automatic run strictly pinned to an offline own-agent runner
    expires too (§9), and a non-strict pin is left to the offline-pin sweep."""
    from pi_dash.managed_runner.tasks import expire_waiting_runs

    mini = _runner(create_user, pod, name="mini", status=RunnerStatus.OFFLINE)
    mini.dev_machine = DevMachine.objects.create(owner=create_user, label="Studio")
    mini.save(update_fields=["dev_machine"])
    strict = _queued(issue, created_by=create_user, pinned_runner=mini, strict=True, trigger=AgentRunTrigger.TICK)
    AgentRun.objects.filter(pk=strict.pk).update(created_at=timezone.now() - timedelta(hours=2))
    assert expire_waiting_runs() == 1
    strict.refresh_from_db()
    assert strict.status == AgentRunStatus.FAILED
    assert strict.error_code == WAITING_FOR_RUNNER
    assert strict.error.startswith("Studio is offline")


# --------------------------------------------------------------------------
# §13 Server data migration
# --------------------------------------------------------------------------


def _data_migration():
    return importlib.import_module("pi_dash.runner.migrations.0035_engine_dispatch_data")


def test_data_migration_executors_backfill_and_prune(workspace, project, pod, create_user, machine, teammate):
    mig = _data_migration()
    state = State.objects.create(name="Todo", group="unstarted", project=project)

    managed_project = Project.objects.create(name="Desk", identifier="DSK", workspace=workspace)
    managed_pod = Pod.default_for_project(managed_project)
    cloud_project = Project.objects.create(name="Cloud", identifier="CLD", workspace=workspace)
    Project.objects.filter(pk=managed_project.pk).update(default_agent_executor="managed_runner")
    Project.objects.filter(pk=cloud_project.pk).update(default_agent_executor="cloud_agent")

    # Engines as the migration finds them: every row on the 0034 default.
    used_engine = _runner(create_user, pod, name="e-used", engine=True, machine=machine, pod_dispatch="pod_member")
    creator_engine = _runner(
        create_user, managed_pod, name="e-creator", engine=True, machine=machine, pod_dispatch="pod_member"
    )
    never_used = _runner(teammate, pod, name="e-never", engine=True, pod_dispatch="pod_member")
    manual = _runner(create_user, pod, name="mini")

    def _issue(name, proj, executor, st=None):
        st = st or State.objects.create(name="S", group="unstarted", project=proj)
        created = Issue.objects.create(workspace=workspace, project=proj, state=st, name=name)
        Issue.objects.filter(pk=created.pk).update(agent_executor=executor, created_by=create_user)
        created.refresh_from_db()
        return created

    from_latest_run = _issue("latest", project, "managed_runner", state)
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=from_latest_run,
        executor_kind="managed_runner",
        pinned_runner=used_engine,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )
    from_creator = _issue("creator", managed_project, "managed_runner")
    nothing = _issue("nothing", cloud_project, "managed_runner")
    cloud_issue = _issue("cloud", project, "cloud_agent", state)

    mig.migrate_executors(django_apps)
    mig.backfill_assigned_runner(django_apps)
    mig.prune_never_used_engine_runners(django_apps)

    for obj in (managed_project, cloud_project, from_latest_run, from_creator, nothing, cloud_issue):
        obj.refresh_from_db()
    for obj in (used_engine, creator_engine, never_used, manual):
        obj.refresh_from_db()

    assert managed_project.default_agent_executor == "local_runner"
    assert cloud_project.default_agent_executor == "local_runner"
    # Engines take exactly what they took before, except in projects whose
    # default was the built-in engine (PO-11).
    assert used_engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    assert creator_engine.pod_dispatch == RunnerPodDispatch.POD_MEMBER
    assert manual.pod_dispatch == RunnerPodDispatch.POD_MEMBER
    # Queued engine-pinned runs become strict.
    assert AgentRun.objects.get(work_item=from_latest_run).pin_strict is True
    # Backfill: latest engine run, else the creator's engine, else pod-level.
    assert (from_latest_run.agent_executor, from_latest_run.assigned_runner_id) == ("local_runner", used_engine.id)
    assert from_latest_run.assigned_pod_id == used_engine.pod_id
    assert (from_creator.assigned_runner_id, from_creator.assigned_pod_id) == (creator_engine.id, managed_pod.id)
    assert (nothing.agent_executor, nothing.assigned_runner_id) == ("local_runner", None)
    assert (cloud_issue.agent_executor, cloud_issue.assigned_runner_id) == ("local_runner", None)
    # Prune: only the engine with no run and no assigned issue.
    assert never_used.revoked_at is not None and never_used.status == RunnerStatus.REVOKED
    assert used_engine.revoked_at is None and creator_engine.revoked_at is None
    assert manual.revoked_at is None

    # A second pass over the issue, run and pruning steps changes nothing.
    mig.backfill_assigned_runner(django_apps)
    mig.prune_never_used_engine_runners(django_apps)
    from_creator.refresh_from_db()
    creator_engine.refresh_from_db()
    assert from_creator.assigned_runner_id == creator_engine.id
    assert creator_engine.revoked_at is None


def test_new_engine_runner_defaults_to_pinned_only(create_user, pod):
    engine = _runner(create_user, pod, name="fresh", engine=True)
    assert engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        assert _runner(create_user, pod, name="own").pod_dispatch == RunnerPodDispatch.POD_MEMBER
