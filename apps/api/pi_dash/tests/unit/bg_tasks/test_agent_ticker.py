# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.bgtasks.agent_ticker``."""

from __future__ import annotations

from datetime import timedelta
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.bgtasks.agent_ticker import fire_tick, scan_due_tickers
from pi_dash.db.models import Issue, Project, State
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
    """Create the issue in Todo (no state-transition signal side effects),
    then transition it to In Progress via ``Issue.all_objects.update`` to
    bypass the post_save handler. Tests can call ``fire_tick`` and the
    scheduling primitives without contention from auto-created runs."""
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


@pytest.fixture
def runner_for_workspace(db, workspace, create_user):
    from pi_dash.runner.models import Pod, Runner, RunnerStatus

    pod = Pod.default_for_workspace(workspace)
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentA",
        credential_hash="h",
        credential_fingerprint="f" * 12,
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


def _make_prior_run(issue, runner):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=runner.owner,
        pod=runner.pod,
        work_item=issue,
        runner=runner,
        thread_id="sess_xyz",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior work",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )


def _make_due_schedule(issue, *, tick_count=0, max_ticks=None):
    sched = scheduling.arm_ticker(issue)
    sched.next_run_at = timezone.now() - timedelta(seconds=1)
    sched.tick_count = tick_count
    if max_ticks is not None:
        sched.max_ticks = max_ticks
    sched.save(
        update_fields=["next_run_at", "tick_count", "max_ticks", "updated_at"]
    )
    return sched


# ---------------------------------------------------------------------------
# scan_due_tickers
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_scan_picks_up_only_due_enabled_under_cap_rows(
    seeded, issue, project, runner_for_workspace
):
    """Three schedules: one due (should fan out), one not due, one disabled."""
    issue2 = Issue.objects.create(
        name="Task2", workspace=issue.workspace, project=project,
        state=issue.state, created_by=issue.created_by,
    )
    issue3 = Issue.objects.create(
        name="Task3", workspace=issue.workspace, project=project,
        state=issue.state, created_by=issue.created_by,
    )
    _make_due_schedule(issue)
    not_due = scheduling.arm_ticker(issue2)
    not_due.next_run_at = timezone.now() + timedelta(hours=2)
    not_due.save(update_fields=["next_run_at"])
    disabled = scheduling.arm_ticker(issue3)
    disabled.next_run_at = timezone.now() - timedelta(seconds=1)
    disabled.enabled = False
    disabled.save(update_fields=["next_run_at", "enabled"])

    with mock.patch(
        "pi_dash.bgtasks.agent_ticker.fire_tick.delay"
    ) as fire:
        count = scan_due_tickers()
    assert count == 1
    assert fire.call_count == 1


# ---------------------------------------------------------------------------
# fire_tick
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fire_tick_increments_tick_count_and_dispatches(
    seeded, issue, runner_for_workspace
):
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)

    fired = fire_tick(str(sched.id))
    assert fired is True

    sched.refresh_from_db()
    assert sched.tick_count == 1
    assert sched.next_run_at > timezone.now()
    assert sched.last_tick_at is not None
    assert sched.enabled is True

    runs = AgentRun.objects.filter(work_item=issue, parent_run__isnull=False)
    assert runs.count() == 1


@pytest.mark.unit
def test_fire_tick_skips_when_already_advanced(seeded, issue, runner_for_workspace):
    """If another fire advances ``next_run_at`` between scan and worker
    pickup, this fire is a no-op."""
    _make_prior_run(issue, runner_for_workspace)
    sched = scheduling.arm_ticker(issue)
    # next_run_at already in the future — fire_tick must not advance.
    sched.next_run_at = timezone.now() + timedelta(hours=2)
    sched.save(update_fields=["next_run_at"])

    fired = fire_tick(str(sched.id))
    assert fired is False
    sched.refresh_from_db()
    assert sched.tick_count == 0


@pytest.mark.unit
def test_fire_tick_disarms_on_cap_hit(
    seeded, issue, runner_for_workspace
):
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, tick_count=23, max_ticks=24)
    fired = fire_tick(str(sched.id))
    assert fired is True

    sched.refresh_from_db()
    assert sched.tick_count == 24
    assert sched.enabled is False


@pytest.mark.unit
def test_fire_tick_does_not_auto_transition_state_immediately(
    seeded, issue, states, runner_for_workspace
):
    """On cap hit, ``fire_tick`` only sets ``enabled = False``. The In
    Progress → Paused transition is deferred to the run-terminate hook."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, tick_count=23, max_ticks=24)
    fire_tick(str(sched.id))
    issue.refresh_from_db()
    assert issue.state == states["in_progress"]


@pytest.mark.unit
def test_fire_tick_skips_when_state_not_in_progress(
    seeded, issue, states, runner_for_workspace
):
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    Issue.all_objects.filter(pk=issue.pk).update(state=states["paused"])

    fired = fire_tick(str(sched.id))
    assert fired is False

    sched.refresh_from_db()
    assert sched.tick_count == 0


@pytest.mark.unit
def test_fire_tick_skips_when_active_run_exists(
    seeded, issue, runner_for_workspace, create_user
):
    """Active-run check happens before tick_count advance — no budget
    consumption when the previous turn is still working."""
    sched = _make_due_schedule(issue)
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
    fired = fire_tick(str(sched.id))
    assert fired is False
    sched.refresh_from_db()
    assert sched.tick_count == 0


@pytest.mark.unit
def test_fire_tick_skips_disabled_schedule(seeded, issue, runner_for_workspace):
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    sched.enabled = False
    sched.save(update_fields=["enabled"])
    fired = fire_tick(str(sched.id))
    assert fired is False
    sched.refresh_from_db()
    assert sched.tick_count == 0
