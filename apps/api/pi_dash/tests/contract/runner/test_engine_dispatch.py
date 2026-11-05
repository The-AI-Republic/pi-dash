# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dispatch core of "the built-in engine as a first-class runner" (PIDESKAPP-123).

Contract tests for ``.ai_design/built_engine_as_runner/design.md`` §6-§9 and
the server data migration of §13 (revision 4.0, pods only): the single Runner
executor with pod-level translation of the legacy values, engine pod
membership (``pod_dispatch``), locality pins to engines, the unchanged
pod-move guard, the machine pause fields, and the engine usage rule in
dispatch and preflight. Nothing here names a runner for an issue: that is
the dedicated-runner design (HA §23).
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
from pi_dash.db.models import Issue, Project, ProjectMember, State, User, WorkspaceMember
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

pytestmark = pytest.mark.contract


# --------------------------------------------------------------------------
# Fixtures
# --------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _no_outbound(monkeypatch):
    """Keep runner frames and Celery tasks in-process.

    ``delay`` is stubbed on the ``Task`` class, not on task instances: an
    instance-level ``monkeypatch.setattr`` leaves the bound method behind as
    an instance attribute on undo, which shadows other tests' class-level
    patches of ``Task.delay``.
    """
    from unittest.mock import MagicMock

    monkeypatch.setattr("pi_dash.runner.services.pubsub.send_to_runner", lambda *a, **k: None)
    with patch("celery.app.task.Task.delay", return_value=MagicMock(id="mock-task-id")):
        yield


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


def _pod_member(runner):
    Runner.objects.filter(pk=runner.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    runner.refresh_from_db()
    return runner


def _queued(
    issue,
    *,
    created_by,
    pinned_runner=None,
    trigger=AgentRunTrigger.RUN_AI,
    pod=None,
    executor_kind=AgentExecutorKind.LOCAL_RUNNER,
):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=created_by,
        pod=pod or (pinned_runner.pod if pinned_runner else issue.assigned_pod),
        work_item=issue,
        pinned_runner=pinned_runner,
        executor_kind=executor_kind,
        trigger=trigger,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )


def _sibling(issue, name="Other task"):
    """Another issue of the same creator and project (one active run per
    issue is a DB constraint)."""
    other = Issue.objects.create(workspace=issue.workspace, project=issue.project, state=issue.state, name=name)
    Issue.objects.filter(pk=other.pk).update(created_by=issue.created_by)
    other.refresh_from_db()
    return other


def _issue_url(workspace, project, issue):
    return f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/{issue.id}/"


# --------------------------------------------------------------------------
# §6 Executor model
# --------------------------------------------------------------------------


def test_executor_options_return_deprecated_entries(session_client, workspace, project, member):
    resp = session_client.get(f"/api/workspaces/{workspace.slug}/projects/{project.id}/")
    assert resp.status_code == 200, resp.content
    options = {opt["kind"]: opt for opt in resp.json()["agent_executor_options"]}
    assert set(options) == {"local_runner", "cloud_agent", "managed_runner"}
    assert options["local_runner"]["deprecated"] is False
    assert options["cloud_agent"]["deprecated"] is True
    assert options["managed_runner"]["deprecated"] is True


@pytest.mark.parametrize("legacy", ["cloud_agent", "managed_runner"])
def test_project_default_executor_write_is_translated(session_client, workspace, project, member, legacy):
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
    assert not hasattr(issue, "assigned_runner_id")


def test_issue_managed_write_from_old_build_is_pod_level(session_client, workspace, project, issue, engine, other_pod):
    """Scenario 16: an old desktop build sets "Pi Dash Agent (desktop)"; the
    server stores ``local_runner`` and keeps the issue's pod. No runner is
    named, even though the caller has an engine in the project."""
    Issue.objects.filter(pk=issue.pk).update(assigned_pod=other_pod)
    resp = session_client.patch(
        _issue_url(workspace, project, issue), {"agent_executor": "managed_runner"}, format="json"
    )
    assert resp.status_code in (200, 204), resp.content
    issue.refresh_from_db()
    assert issue.agent_executor == AgentExecutorKind.LOCAL_RUNNER
    assert issue.assigned_pod_id == other_pod.id
    fields = execution_fields(project=issue.project, run_kind="issue", has_issue=True, actor=engine.owner)
    assert fields == {"executor_kind": AgentExecutorKind.LOCAL_RUNNER, "tool_plan": {}}


def test_issue_executor_write_refused_under_a_live_run(session_client, workspace, project, issue, create_user, pod):
    Issue.objects.filter(pk=issue.pk).update(agent_executor=AgentExecutorKind.CLOUD_AGENT)
    issue.refresh_from_db()
    _queued(issue, created_by=create_user, pod=pod)
    resp = session_client.patch(
        _issue_url(workspace, project, issue), {"agent_executor": "local_runner"}, format="json"
    )
    assert resp.status_code == 400, resp.content
    assert "agent_executor" in resp.json()


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
# §7 Pods, pod work and the pod-move guard
# --------------------------------------------------------------------------


def test_pinned_only_engine_takes_no_pod_work(issue, engine, pod, create_user):
    run = _queued(issue, created_by=create_user, pod=pod)
    assert engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    with transaction.atomic():
        assert matcher.next_for_runner(engine) is None
        assert matcher.select_runner_in_pod(pod) is None
    _pod_member(engine)
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


def test_pod_member_engine_ranks_equally(issue, engine, pod, create_user):
    """PO-4: an idle pod-member engine is a candidate like any own-agent
    runner; the matcher's existing order picks between them."""
    _pod_member(engine)
    mini = _runner(create_user, pod, name="mini")
    candidates = set(
        Runner.objects.assignable().filter(pod=pod).exclude(pod_dispatch=RunnerPodDispatch.PINNED_ONLY)
    )
    assert candidates == {engine, mini}
    run = _queued(issue, created_by=create_user, pod=pod)
    assert matcher.drain_pod(pod) == 1
    run.refresh_from_db()
    assert run.runner_id in (engine.id, mini.id)


def test_legacy_managed_run_is_served_only_to_its_engine(issue, engine, pod, create_user):
    """§13: queued legacy ``managed_runner`` runs keep their pin and are served
    to that engine, even while it is ``pinned_only``; nothing else takes
    them."""
    legacy = _queued(
        issue, created_by=create_user, pinned_runner=engine, executor_kind=AgentExecutorKind.MANAGED_RUNNER
    )
    mini = _runner(create_user, pod, name="mini")
    with transaction.atomic():
        assert matcher.next_for_runner(mini) is None
        assert matcher.next_for_runner(engine).id == legacy.id


def test_own_agent_runner_never_takes_unpinned_legacy_managed_runs(issue, pod, create_user):
    _queued(issue, created_by=create_user, pod=pod, executor_kind=AgentExecutorKind.MANAGED_RUNNER)
    mini = _runner(create_user, pod, name="mini")
    with transaction.atomic():
        assert matcher.next_for_runner(mini) is None


def test_pod_move_refused_while_a_run_is_pinned_to_the_runner(session_client, issue, engine, other_pod, create_user):
    """Scenario 9: the guard is main's ``409 runner_busy``; no run's ``pod``
    is ever rewritten. After the run is released, the move succeeds."""
    _pod_member(engine)
    run = _queued(issue, created_by=create_user, pinned_runner=engine)
    url = reverse("runner-detail", kwargs={"runner_id": engine.id})
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 409, resp.content
    assert resp.json()["code"] == "runner_busy"
    run.refresh_from_db()
    assert run.pod_id == engine.pod_id

    resp = session_client.post(reverse("runner-run-release-pin", kwargs={"run_id": run.id}))
    assert resp.status_code == 200, resp.content
    resp = session_client.patch(url, {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 200, resp.content
    old_pod = engine.pod_id
    engine.refresh_from_db()
    run.refresh_from_db()
    assert engine.pod_id == other_pod.id
    assert run.pod_id == old_pod


def test_pod_dispatch_rejects_unknown_values(session_client, engine):
    resp = session_client.patch(
        reverse("runner-detail", kwargs={"runner_id": engine.id}), {"pod_dispatch": "dedicated"}, format="json"
    )
    assert resp.status_code == 400, resp.content


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
# §7 Locality pins to engines
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

    continuation = _queued(issue, created_by=create_user, pinned_runner=engine, pod=pod)
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == continuation.id

    resp = session_client.patch(url, {"pod_dispatch": "pinned_only"}, format="json")
    assert resp.status_code == 200, resp.content
    continuation.refresh_from_db()
    assert continuation.pinned_runner_id is None
    assert continuation.pod_id == pod.id


def test_continuation_is_not_pinned_to_a_paused_pod_member_engine(issue, engine, machine, pod, create_user):
    from pi_dash.orchestration.service import _pinned_runner_for

    _pod_member(engine)
    parent = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        runner=engine,
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    assert _pinned_runner_for(parent, pod) == engine
    DevMachine.objects.filter(pk=machine.pk).update(engine_paused_by_owner=True)
    parent.refresh_from_db()
    assert _pinned_runner_for(parent, pod) is None


def test_owner_pause_releases_locality_pins_and_blocks_matching(
    session_client, workspace, issue, engine, machine, pod, create_user
):
    _pod_member(engine)
    nonstrict = _queued(issue, created_by=create_user, pinned_runner=engine, pod=pod)
    legacy = _queued(
        _sibling(issue),
        created_by=create_user,
        pinned_runner=engine,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
    )
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = session_client.patch(url, {"workspace": str(workspace.id), "engine_paused_by_owner": True}, format="json")
    assert resp.status_code == 200, resp.content
    assert resp.json()["engine_paused_by_owner"] is True
    nonstrict.refresh_from_db()
    legacy.refresh_from_db()
    assert nonstrict.pinned_runner_id is None
    assert nonstrict.pod_id == pod.id
    # Only that engine serves a legacy managed run, so its pin stays.
    assert legacy.pinned_runner_id == engine.id
    assert not Runner.objects.assignable().filter(pk=engine.pk).exists()

    resp = session_client.patch(url, {"workspace": str(workspace.id), "engine_paused_by_owner": False}, format="json")
    assert resp.status_code == 200
    assert Runner.objects.assignable().filter(pk=engine.pk).exists()


def test_auto_pause_reason_blocks_matching_and_releases_pins(issue, engine, machine, pod, create_user):
    from pi_dash.runner.services.dispatch_rules import set_engine_auto_pause_reason

    _pod_member(engine)
    pinned = _queued(issue, created_by=create_user, pinned_runner=engine, pod=pod)
    assert set_engine_auto_pause_reason(machine, "wallet_empty") is True
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id is None
    assert not Runner.objects.assignable().filter(pk=engine.pk).exists()
    with pytest.raises(ValueError):
        set_engine_auto_pause_reason(machine, "bogus")
    assert set_engine_auto_pause_reason(machine, "") is True
    assert Runner.objects.assignable().filter(pk=engine.pk).exists()


def test_machine_patch_is_owner_only_and_engine_fields_only(api_client, workspace, machine, teammate):
    api_client.force_authenticate(user=teammate)
    url = reverse("dev-machine-delete", kwargs={"machine_id": machine.id})
    resp = api_client.patch(url, {"workspace": str(workspace.id), "engine_team_runs": True}, format="json")
    assert resp.status_code == 404
    machine.refresh_from_db()
    assert machine.engine_team_runs is False


def test_offline_sweep_releases_locality_pin_on_pinned_only_engine(issue, engine, pod, create_user):
    """Backstop: a locality pin left on an engine whose pod work is off goes
    back to the pod; a legacy managed run pinned to it does not."""
    from pi_dash.runner.tasks import release_pins_on_offline_runners

    pinned = _queued(issue, created_by=create_user, pinned_runner=engine, pod=pod)
    legacy = _queued(
        _sibling(issue),
        created_by=create_user,
        pinned_runner=engine,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
    )
    AgentRun.objects.filter(pk__in=[pinned.pk, legacy.pk]).update(created_at=timezone.now() - timedelta(hours=1))
    with override_settings(RUNNER_OFFLINE_PIN_RELEASE_SECS=60):
        assert release_pins_on_offline_runners() == 1
    pinned.refresh_from_db()
    legacy.refresh_from_db()
    assert pinned.pinned_runner_id is None
    assert legacy.pinned_runner_id == engine.id


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
    """Scenario 7."""
    _pod_member(engine)
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
    _pod_member(engine)
    run = _queued(issue, created_by=create_bot_user, trigger=AgentRunTrigger.TICK, pod=pod)
    with transaction.atomic():
        assert matcher.next_for_runner(engine).id == run.id


def test_preflight_bounce_does_not_count_engine_for_teammate_click(issue, engine, pod, teammate):
    Runner.objects.filter(pk=engine.pk).update(pod_dispatch=RunnerPodDispatch.POD_MEMBER)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, teammate.id, human=True) is False
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is True


def test_teammate_run_on_someone_elses_issue_not_admitted_even_with_opt_in(issue, engine, machine, pod, teammate):
    """The opt-in restores the ordinary rule, which still needs the owner to
    be involved in the issue."""
    _pod_member(engine)
    other = Issue.objects.create(workspace=issue.workspace, project=issue.project, state=issue.state, name="Theirs")
    Issue.objects.filter(pk=other.pk).update(created_by=teammate)
    other.refresh_from_db()
    _queued(other, created_by=teammate, pod=pod)
    DevMachine.objects.filter(pk=machine.pk).update(engine_team_runs=True)
    engine.refresh_from_db()
    with transaction.atomic():
        assert matcher.next_for_runner(engine) is None


def test_tick_preflight_counts_pod_member_engine_only(issue, engine, pod, create_user):
    """Scenario 20: a tick on the owner's issue where the owner's engine is
    the only member passes preflight; with the engine ``pinned_only`` it
    bounces."""
    assert engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is False
    _pod_member(engine)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, None, human=False) is True


def test_preflight_counts_offline_pod_member_engine(issue, engine, pod, create_user):
    """Scenario 10: an offline engine is an offline runner; the run waits in
    the pod rather than the issue bouncing."""
    _pod_member(engine)
    Runner.objects.filter(pk=engine.pk).update(status=RunnerStatus.OFFLINE, last_heartbeat_at=None)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, create_user.id, human=True) is True


def test_preflight_bounce_on_teammate_state_move(issue, engine, machine, pod, teammate):
    """Scenario 24: a teammate's human event in a pod whose only member is
    the owner's engine bounces; with the opt-in on, it does not."""
    from pi_dash.orchestration import scheduling

    _pod_member(engine)
    with patch.object(scheduling, "_bounce_issue_no_eligible_runner") as bounce:
        assert not scheduling.preflight_eligibility_or_bounce(
            issue, run_creator=teammate, pod=pod, triggered_by=AgentRunTrigger.STATE_TRANSITION.value
        )
    bounce.assert_called_once()
    DevMachine.objects.filter(pk=machine.pk).update(engine_team_runs=True)
    with patch.object(scheduling, "_bounce_issue_no_eligible_runner") as bounce:
        assert scheduling.preflight_eligibility_or_bounce(
            issue, run_creator=teammate, pod=pod, triggered_by=AgentRunTrigger.STATE_TRANSITION.value
        )
    bounce.assert_not_called()


# --------------------------------------------------------------------------
# §13 Server data migration
# --------------------------------------------------------------------------


def _data_migration():
    return importlib.import_module("pi_dash.runner.migrations.0035_engine_dispatch_data")


def test_data_migration_executors_and_prune(workspace, project, pod, other_pod, create_user, machine, teammate):
    mig = _data_migration()
    state = State.objects.create(name="Todo", group="unstarted", project=project)

    managed_project = Project.objects.create(name="Desk", identifier="DSK", workspace=workspace)
    managed_pod = Pod.default_for_project(managed_project)
    cloud_project = Project.objects.create(name="Cloud", identifier="CLD", workspace=workspace)
    Project.objects.filter(pk=managed_project.pk).update(default_agent_executor="managed_runner")
    Project.objects.filter(pk=cloud_project.pk).update(default_agent_executor="cloud_agent")

    # Engines as the migration finds them: every row on the 0034 default.
    used_engine = _runner(create_user, pod, name="e-used", engine=True, machine=machine, pod_dispatch="pod_member")
    managed_engine = _runner(
        create_user, managed_pod, name="e-managed", engine=True, machine=machine, pod_dispatch="pod_member"
    )
    never_used = _runner(teammate, pod, name="e-never", engine=True, pod_dispatch="pod_member")
    manual = _runner(create_user, pod, name="mini")

    def _issue(name, proj, executor, st=None, assigned_pod=None):
        st = st or State.objects.create(name="S", group="unstarted", project=proj)
        created = Issue.objects.create(workspace=workspace, project=proj, state=st, name=name)
        Issue.objects.filter(pk=created.pk).update(
            agent_executor=executor, created_by=create_user, assigned_pod=assigned_pod
        )
        created.refresh_from_db()
        return created

    managed_issue = _issue("managed", project, "managed_runner", state, assigned_pod=other_pod)
    legacy_run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=managed_issue,
        executor_kind="managed_runner",
        pinned_runner=used_engine,
        status=AgentRunStatus.QUEUED,
        prompt="x",
    )
    # Gives ``managed_engine`` a (finished) run, so it is not pruned.
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=managed_pod,
        runner=managed_engine,
        executor_kind="managed_runner",
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    managed_no_pod = _issue("managed-no-pod", managed_project, "managed_runner")
    cloud_issue = _issue("cloud", project, "cloud_agent", state, assigned_pod=pod)

    mig.migrate_executors(django_apps)
    mig.migrate_issue_executors(django_apps)
    mig.prune_never_used_engine_runners(django_apps)

    for obj in (managed_project, cloud_project, managed_issue, managed_no_pod, cloud_issue, legacy_run):
        obj.refresh_from_db()
    for obj in (used_engine, managed_engine, never_used, manual):
        obj.refresh_from_db()

    assert managed_project.default_agent_executor == "local_runner"
    assert cloud_project.default_agent_executor == "local_runner"
    # Engines take no new issue work, except in projects whose default was
    # the built-in engine (PO-11).
    assert used_engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    assert managed_engine.pod_dispatch == RunnerPodDispatch.POD_MEMBER
    assert manual.pod_dispatch == RunnerPodDispatch.POD_MEMBER
    # Issues become pod-level ``local_runner``: their pod is kept, or the
    # project's default pod when they had none.
    assert (managed_issue.agent_executor, managed_issue.assigned_pod_id) == ("local_runner", other_pod.id)
    assert (managed_no_pod.agent_executor, managed_no_pod.assigned_pod_id) == ("local_runner", managed_pod.id)
    assert (cloud_issue.agent_executor, cloud_issue.assigned_pod_id) == ("local_runner", pod.id)
    # Runs are not touched: the legacy run keeps its kind, pin and pod.
    assert legacy_run.executor_kind == "managed_runner"
    assert (legacy_run.pinned_runner_id, legacy_run.pod_id) == (used_engine.id, pod.id)
    # Prune: only the engine with no run at all.
    assert never_used.revoked_at is not None and never_used.status == RunnerStatus.REVOKED
    assert used_engine.revoked_at is None and managed_engine.revoked_at is None
    assert manual.revoked_at is None

    # A second pass over the issue and pruning steps changes nothing.
    mig.migrate_issue_executors(django_apps)
    mig.prune_never_used_engine_runners(django_apps)
    managed_issue.refresh_from_db()
    used_engine.refresh_from_db()
    assert managed_issue.assigned_pod_id == other_pod.id
    assert used_engine.revoked_at is None


def test_new_engine_runner_defaults_to_pinned_only(create_user, pod):
    engine = _runner(create_user, pod, name="fresh", engine=True)
    assert engine.pod_dispatch == RunnerPodDispatch.PINNED_ONLY
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        assert _runner(create_user, pod, name="own").pod_dispatch == RunnerPodDispatch.POD_MEMBER
