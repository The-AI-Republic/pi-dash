# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.orchestration.scheduling`` (PR B/C)."""

from __future__ import annotations

from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.orchestration import scheduling
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )


@pytest.fixture
def states(project, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project, group="unstarted"
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, group="started"
            ),
            "paused": State.objects.create(
                name="Paused", project=project, group="backlog"
            ),
            "done": State.objects.create(
                name="Done", project=project, group="completed"
            ),
        }


@pytest.fixture
def issue(workspace, project, states, create_user):
    """Create the issue in Todo so the state-transition signal does not
    auto-create an ``AgentRun`` or auto-arm a schedule. Tests that need
    the issue in In Progress should transition it explicitly via
    ``Issue.all_objects.filter(...).update(...)`` (which bypasses the
    signal) and then call the scheduling primitives directly."""
    with impersonate(create_user):
        return Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )


def _to_in_progress(issue, states):
    """Bypass the post_save signal so tests stay deterministic."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()


@pytest.fixture
def runner_for_workspace(db, workspace, project, create_user):
    from pi_dash.runner.models import Pod, Runner, RunnerStatus

    pod = Pod.default_for_project(project)
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentA",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def stub_drain(monkeypatch):
    from pi_dash.runner.services import matcher

    drain_mock = mock.Mock()
    monkeypatch.setattr(matcher, "drain_pod_by_id", drain_mock)
    monkeypatch.setattr(
        "django.db.transaction.on_commit",
        lambda fn, **kw: fn(),
    )
    return drain_mock


# ---------------------------------------------------------------------------
# arm_ticker
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_arm_ticker_creates_row_and_sets_next_run_at(seeded, issue, states):
    _to_in_progress(issue, states)
    sched = scheduling.arm_ticker(issue)
    assert sched.tick_count == 0
    assert sched.next_run_at is not None
    assert sched.next_run_at > timezone.now()
    assert sched.enabled is True


@pytest.mark.unit
def test_arm_ticker_is_idempotent_resets_tick_count(seeded, issue, states):
    _to_in_progress(issue, states)
    sched = scheduling.arm_ticker(issue)
    sched.tick_count = 5
    sched.save(update_fields=["tick_count"])
    again = scheduling.arm_ticker(issue)
    assert again.tick_count == 0
    assert again.pk == sched.pk  # one row per issue


@pytest.mark.unit
def test_arm_ticker_respects_user_disabled(seeded, issue, states):
    _to_in_progress(issue, states)
    IssueAgentTicker.objects.create(issue=issue, user_disabled=True)
    sched = scheduling.arm_ticker(issue)
    assert sched.user_disabled is True
    assert sched.enabled is False


@pytest.mark.unit
def test_arm_ticker_respects_project_disabled(seeded, issue, project, states):
    _to_in_progress(issue, states)
    project.agent_ticking_enabled = False
    project.save(update_fields=["agent_ticking_enabled"])
    sched = scheduling.arm_ticker(issue)
    assert sched.enabled is False


@pytest.mark.unit
def test_arm_ticker_dispatch_immediate_false_does_not_dispatch(
    seeded, issue, states, runner_for_workspace
):
    """``dispatch_immediate=False`` is a documentation flag — arming itself
    never creates a run regardless of value."""
    _to_in_progress(issue, states)
    scheduling.arm_ticker(issue, dispatch_immediate=False)
    assert AgentRun.objects.filter(work_item=issue).count() == 0


# ---------------------------------------------------------------------------
# disarm_ticker
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_disarm_ticker_idempotent(seeded, issue, states):
    _to_in_progress(issue, states)
    scheduling.arm_ticker(issue)
    scheduling.disarm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    assert sched.enabled is False
    # Calling again is a no-op.
    scheduling.disarm_ticker(issue)
    sched.refresh_from_db()
    assert sched.enabled is False


@pytest.mark.unit
def test_disarm_ticker_with_no_row_returns_none(seeded, issue):
    """No state transition has happened, so no schedule row exists yet."""
    assert IssueAgentTicker.objects.filter(issue=issue).exists() is False
    assert scheduling.disarm_ticker(issue) is None


# ---------------------------------------------------------------------------
# reset_ticker_after_comment_and_run
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_reset_after_comment_and_run_resets_tick_count(seeded, issue, states):
    _to_in_progress(issue, states)
    sched = scheduling.arm_ticker(issue)
    sched.tick_count = 17
    sched.save(update_fields=["tick_count"])
    out = scheduling.reset_ticker_after_comment_and_run(issue)
    assert out.tick_count == 0
    assert out.next_run_at > timezone.now()


# ---------------------------------------------------------------------------
# dispatch_continuation_run
# ---------------------------------------------------------------------------


def _make_paused_run(issue, runner, *, thread_id="sess_xyz"):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=runner.owner,
        pod=runner.pod,
        work_item=issue,
        runner=runner,
        thread_id=thread_id,
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior work",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )


@pytest.mark.unit
def test_dispatch_continuation_run_blocked_by_active_run(
    seeded, issue, states, runner_for_workspace, create_user
):
    _to_in_progress(issue, states)
    AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        runner=runner_for_workspace,
        status=AgentRunStatus.RUNNING,
        prompt="working",
        started_at=timezone.now(),
    )
    run = scheduling.dispatch_continuation_run(
        issue, triggered_by=scheduling.TRIGGER_TICK
    )
    assert run is None


@pytest.mark.unit
def test_dispatch_continuation_run_skips_when_no_prior(seeded, issue, states):
    _to_in_progress(issue, states)
    run = scheduling.dispatch_continuation_run(
        issue, triggered_by=scheduling.TRIGGER_TICK
    )
    assert run is None


@pytest.mark.unit
def test_dispatch_continuation_run_creates_pinned_continuation(
    seeded, issue, states, runner_for_workspace
):
    _to_in_progress(issue, states)
    prior = _make_paused_run(issue, runner_for_workspace)
    run = scheduling.dispatch_continuation_run(
        issue, triggered_by=scheduling.TRIGGER_TICK
    )
    assert run is not None
    assert run.parent_run_id == prior.pk
    assert run.status == AgentRunStatus.QUEUED


# ---------------------------------------------------------------------------
# maybe_apply_deferred_pause
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_deferred_pause_skips_when_schedule_still_enabled(
    seeded, issue, states, runner_for_workspace, create_user
):
    _to_in_progress(issue, states)
    scheduling.arm_ticker(issue)
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    applied = scheduling.maybe_apply_deferred_pause(run)
    assert applied is False
    issue.refresh_from_db()
    assert issue.state == states["in_progress"]


@pytest.mark.unit
def test_deferred_pause_applies_when_disarmed_and_no_active_runs(
    seeded, issue, states, runner_for_workspace, create_user
):
    _to_in_progress(issue, states)
    sched = scheduling.arm_ticker(issue)
    sched.enabled = False
    sched.save(update_fields=["enabled"])
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        status=AgentRunStatus.COMPLETED,
        prompt="x",
    )
    applied = scheduling.maybe_apply_deferred_pause(run)
    assert applied is True
    issue.refresh_from_db()
    assert issue.state == states["paused"]


@pytest.mark.unit
def test_deferred_pause_skips_when_other_active_run_exists(
    seeded, issue, states, runner_for_workspace, create_user
):
    _to_in_progress(issue, states)
    sched = scheduling.arm_ticker(issue)
    sched.enabled = False
    sched.save(update_fields=["enabled"])
    AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        prompt="still working",
        started_at=timezone.now(),
    )
    terminated = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        status=AgentRunStatus.COMPLETED,
        prompt="finished",
    )
    applied = scheduling.maybe_apply_deferred_pause(terminated)
    assert applied is False
    issue.refresh_from_db()
    assert issue.state == states["in_progress"]
