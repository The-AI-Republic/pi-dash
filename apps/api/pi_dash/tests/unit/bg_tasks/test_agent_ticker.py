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
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
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
            "in_review": State.objects.create(
                name="In Review", project=project, group="review"
            ),
            "in_test": State.objects.create(
                name="In Test", project=project, group="test"
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


def _set_pool(project, pool):
    project.agent_default_max_ticks = pool
    project.save(update_fields=["agent_default_max_ticks"])


def _make_due_schedule(issue, *, used=0, pool=None):
    if pool is not None:
        _set_pool(issue.project, pool)
    sched = scheduling.arm_ticker(issue)
    sched.next_run_at = timezone.now() - timedelta(seconds=1)
    sched.used = used
    sched.save(update_fields=["next_run_at", "used", "updated_at"])
    return sched


def _make_issue_in(states, key, project, workspace, create_user, name):
    with impersonate(create_user):
        i = Issue.objects.create(
            name=name,
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states[key])
    i.refresh_from_db()
    return i


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


@pytest.mark.unit
def test_scan_measures_every_stage_against_the_one_pool(
    seeded, project, states, workspace, create_user
):
    """One pool per issue: an In Review row and an In Test row are admitted
    or filtered by ``used < pool + granted`` exactly like an In Progress
    row — there is no per-stage cap for the scan to pick."""
    _set_pool(project, 10)
    review_issue = _make_issue_in(states, "in_review", project, workspace, create_user, "Review")
    test_issue = _make_issue_in(states, "in_test", project, workspace, create_user, "Test")
    for issue_, used in ((review_issue, 10), (test_issue, 9)):
        sched = scheduling.arm_ticker(issue_)
        sched.next_run_at = timezone.now() - timedelta(seconds=1)
        sched.used = used
        sched.save(update_fields=["next_run_at", "used", "updated_at"])

    with mock.patch("pi_dash.bgtasks.agent_ticker.fire_tick.delay") as fire:
        count = scan_due_tickers()
    # review at 10/10 filtered; test at 9/10 admitted
    assert count == 1
    fired_ids = {call.args[0] for call in fire.call_args_list}
    assert fired_ids == {str(IssueAgentTicker.objects.get(issue=test_issue).id)}


@pytest.mark.unit
def test_scan_counts_retick_grants_toward_the_cap(
    seeded, issue, project
):
    _set_pool(project, 10)
    sched = _make_due_schedule(issue, used=10)
    sched.granted = 3
    sched.save(update_fields=["granted"])
    with mock.patch("pi_dash.bgtasks.agent_ticker.fire_tick.delay") as fire:
        assert scan_due_tickers() == 1
    assert fire.call_count == 1


@pytest.mark.unit
def test_scan_admits_a_pending_entry_on_a_spent_pool(
    seeded, issue, project
):
    """A human's free entry run must fire even when the pool is spent
    (design §4.5 / §5.2); the scan admits pending rows regardless of cap."""
    _set_pool(project, 10)
    sched = _make_due_schedule(issue, used=10)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.save(update_fields=["pending_entry", "pending_entry_free"])
    with mock.patch("pi_dash.bgtasks.agent_ticker.fire_tick.delay") as fire:
        assert scan_due_tickers() == 1
    assert fire.call_count == 1


@pytest.mark.unit
def test_scan_admits_infinite_pool(seeded, issue, project):
    _set_pool(project, -1)
    _make_due_schedule(issue, used=500)
    with mock.patch("pi_dash.bgtasks.agent_ticker.fire_tick.delay") as fire:
        assert scan_due_tickers() == 1
    assert fire.call_count == 1


@pytest.mark.unit
def test_cadence_registry_covers_every_ticking_phase():
    """Every phase resolves an interval column; budget is not per phase."""
    from pi_dash.orchestration.agent_phases import (
        PHASES,
        cadence_fields_by_group,
    )

    by_group = cadence_fields_by_group()
    assert set(by_group) == set(PHASES)
    columns = [f.project_interval for f in by_group.values()]
    assert len(set(columns)) == len(columns)


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
    assert sched.used == 1
    assert sched.next_run_at > timezone.now()
    assert sched.last_tick_at is not None
    assert sched.enabled is True

    runs = AgentRun.objects.filter(work_item=issue, parent_run__isnull=False)
    assert runs.count() == 1
    # The trigger is persisted on the run and rendered into the prompt so
    # the agent knows a scheduled tick (not a human) woke it.
    run = runs.get()
    assert run.trigger == "tick"
    assert run.phase_kind == "coding-task"
    assert "automatically by the issue's ticker" in run.prompt
    assert "Runs used on this issue" in run.prompt
    assert "1 of 10" in run.prompt


@pytest.mark.unit
def test_fire_tick_dispatches_for_in_test_issue(
    seeded, issue, states, runner_for_workspace
):
    """``fire_tick`` ticks an In Test issue — In Test is a registered
    ticking phase, so ``is_ticking_state`` passes and the continuation
    run dispatches, exactly like In Progress / In Review."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_test"])
    issue.refresh_from_db()
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)

    fired = fire_tick(str(sched.id))
    assert fired is True

    sched.refresh_from_db()
    assert sched.used == 1
    runs = AgentRun.objects.filter(work_item=issue).exclude(prompt="prior work")
    assert runs.count() == 1
    run = runs.get()
    assert run.trigger == "tick"
    # The prompt kind is resolved from the state *at claim time* — and
    # because the prior run was an implementation run, the test entry is a
    # fresh session (no parent), exactly like a transition dispatch.
    assert run.phase_kind == "test"
    assert run.parent_run_id is None


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
    assert sched.used == 0


@pytest.mark.unit
def test_fire_tick_disarms_on_cap_hit(
    seeded, issue, runner_for_workspace
):
    from pi_dash.db.models.issue_agent_ticker import TickerDisarmReason

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=9, pool=10)
    fired = fire_tick(str(sched.id))
    assert fired is True

    sched.refresh_from_db()
    assert sched.used == 10
    assert sched.enabled is False
    # Cap-hit disarm must persist the reason so ``maybe_apply_deferred_pause``
    # can distinguish it from terminal-signal disarms (PR A §4.5 / §6.3).
    assert sched.disarm_reason == TickerDisarmReason.CAP_HIT


@pytest.mark.unit
def test_fire_tick_does_not_auto_transition_state_immediately(
    seeded, issue, states, runner_for_workspace
):
    """On cap hit, ``fire_tick`` only sets ``enabled = False``. The In
    Progress → Paused transition is deferred to the run-terminate hook."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=9, pool=10)
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
    assert sched.used == 0


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
    assert sched.used == 0
    # The pending-entry queue survives the skip: the scanner retries.
    assert sched.next_run_at <= timezone.now()


@pytest.mark.unit
def test_fire_tick_skips_disabled_schedule(seeded, issue, runner_for_workspace):
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    sched.enabled = False
    sched.save(update_fields=["enabled"])
    fired = fire_tick(str(sched.id))
    assert fired is False
    sched.refresh_from_db()
    assert sched.used == 0


# ---------------------------------------------------------------------------
# The entry-run queue (design §4.5) and free claims (§5.2)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fire_tick_free_pending_entry_does_not_spend_the_pool(
    seeded, issue, runner_for_workspace
):
    """A human-started run that had to wait for the issue to be free is
    claimed by ``fire_tick`` but must not count, and renders as a human
    trigger."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=4)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.save(update_fields=["pending_entry", "pending_entry_free"])

    assert fire_tick(str(sched.id)) is True
    sched.refresh_from_db()
    assert sched.used == 4
    assert sched.pending_entry is False
    assert sched.pending_entry_free is False
    assert sched.enabled is True
    run = AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).get()
    assert run.trigger == "run_ai"


@pytest.mark.unit
def test_fire_tick_free_pending_entry_fires_on_a_spent_pool_then_stops(
    seeded, issue, runner_for_workspace
):
    """Free runs fire even when the pool is spent; afterwards the clock is
    stopped again with ``cap_hit`` so no timer tick follows."""
    from pi_dash.db.models.issue_agent_ticker import TickerDisarmReason

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=10, pool=10)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.save(update_fields=["pending_entry", "pending_entry_free"])

    assert fire_tick(str(sched.id)) is True
    sched.refresh_from_db()
    assert sched.used == 10
    assert sched.enabled is False
    # POOL_SPENT, not CAP_HIT: this free run must not end in an auto-Pause.
    assert sched.disarm_reason == TickerDisarmReason.POOL_SPENT
    assert AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).count() == 1


@pytest.mark.unit
def test_fire_tick_free_pending_entry_is_created_as_the_person_who_asked(
    seeded, issue, runner_for_workspace, create_user
):
    """The queued human lever fires as that person, with their trigger —
    exactly as if it had dispatched immediately."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=4)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.pending_entry_actor = create_user
    sched.pending_entry_trigger = "comment_and_run"
    sched.save(update_fields=["pending_entry", "pending_entry_free", "pending_entry_actor", "pending_entry_trigger"])

    assert fire_tick(str(sched.id)) is True
    sched.refresh_from_db()
    assert sched.pending_entry_actor_id is None
    assert sched.pending_entry_trigger == ""
    run = AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).get()
    assert run.trigger == "comment_and_run"
    assert run.created_by_id == create_user.id


@pytest.mark.unit
def test_fire_tick_agent_queued_entry_spends_the_pool(
    seeded, issue, runner_for_workspace
):
    """The entry run an agent's own move queued is machine-started: it
    counts, and renders as a tick."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=4)
    sched.pending_entry = True
    sched.pending_entry_free = False
    sched.save(update_fields=["pending_entry", "pending_entry_free"])

    assert fire_tick(str(sched.id)) is True
    sched.refresh_from_db()
    assert sched.used == 5
    assert sched.pending_entry is False
    run = AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).get()
    assert run.trigger == "tick"


@pytest.mark.unit
def test_fire_tick_rolls_back_pending_flags_when_dispatch_fails(
    seeded, issue, runner_for_workspace
):
    """A claim whose dispatch returns None restores the queue so the entry
    is not lost."""
    from pi_dash.bgtasks import agent_ticker as ticker_mod

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=4)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.save(update_fields=["pending_entry", "pending_entry_free"])

    with mock.patch.object(
        ticker_mod, "fire_tick", wraps=ticker_mod.fire_tick
    ), mock.patch(
        "pi_dash.orchestration.scheduling.dispatch_continuation_run", return_value=None
    ):
        assert fire_tick(str(sched.id)) is False
    sched.refresh_from_db()
    assert sched.used == 4
    assert sched.pending_entry is True
    assert sched.pending_entry_free is True


@pytest.mark.unit
def test_fire_tick_bail_on_a_queued_entry_parks_as_pool_spent(seeded, issue, runner_for_workspace):
    """The pool was lowered after an agent's move queued a counting entry:
    the entry consumed nothing, so the clock stops as pool_spent — not the
    auto-pausing cap_hit — and the issue stays where Re-tick can reach it."""
    from pi_dash.db.models.issue_agent_ticker import TickerDisarmReason

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue, used=8, pool=8)
    sched.pending_entry = True
    sched.pending_entry_free = False
    sched.save(update_fields=["pending_entry", "pending_entry_free"])
    assert fire_tick(str(sched.id)) is False
    sched.refresh_from_db()
    assert sched.used == 8
    assert sched.enabled is False
    assert sched.disarm_reason == TickerDisarmReason.POOL_SPENT
    assert sched.pending_entry is False


# ---------------------------------------------------------------------------
# The switches stop an already-armed clock (project settings / user disable)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fire_tick_does_not_fire_when_project_ticking_is_switched_off(
    seeded, issue, project, runner_for_workspace
):
    """Turning the project switch off stops rows that were already armed.

    Nothing walks the ticker table when the flag flips, so a row armed while
    ticking was on is still due afterwards. It must disarm without spending a
    tick or dispatching a run — otherwise every armed issue in the project
    gets one more automatic agent run after the admin turned ticking off.
    """
    from pi_dash.db.models.issue_agent_ticker import TickerDisarmReason

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    project.agent_ticking_enabled = False
    project.save(update_fields=["agent_ticking_enabled"])

    assert fire_tick(str(sched.id)) is False

    sched.refresh_from_db()
    assert sched.used == 0
    assert sched.enabled is False
    assert sched.disarm_reason == TickerDisarmReason.NONE
    assert AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).count() == 0


@pytest.mark.unit
def test_fire_tick_does_not_fire_an_agent_queued_entry_when_ticking_is_off(
    seeded, issue, project, runner_for_workspace
):
    """A counting (agent-queued) entry is a machine-started run, so the
    project switch stops it too — only a human's free entry gets through."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    sched.pending_entry = True
    sched.pending_entry_free = False
    sched.save(update_fields=["pending_entry", "pending_entry_free"])
    project.agent_ticking_enabled = False
    project.save(update_fields=["agent_ticking_enabled"])

    assert fire_tick(str(sched.id)) is False

    sched.refresh_from_db()
    assert sched.used == 0
    assert sched.enabled is False
    assert sched.pending_entry is False
    assert AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).count() == 0


@pytest.mark.unit
def test_fire_tick_free_pending_entry_still_fires_when_ticking_is_off(
    seeded, issue, project, runner_for_workspace
):
    """The human asked for this run before the switch was flipped, so it
    fires — but the clock is left disarmed so no timer tick follows it."""
    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    sched.pending_entry = True
    sched.pending_entry_free = True
    sched.save(update_fields=["pending_entry", "pending_entry_free"])
    project.agent_ticking_enabled = False
    project.save(update_fields=["agent_ticking_enabled"])

    assert fire_tick(str(sched.id)) is True

    sched.refresh_from_db()
    assert sched.used == 0
    assert sched.enabled is False
    assert AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).count() == 1


@pytest.mark.unit
def test_fire_tick_does_not_fire_a_user_disabled_row(
    seeded, issue, runner_for_workspace
):
    """``user_disabled`` is the per-issue switch; same rule as the project one."""
    from pi_dash.db.models.issue_agent_ticker import TickerDisarmReason

    _make_prior_run(issue, runner_for_workspace)
    sched = _make_due_schedule(issue)
    sched.user_disabled = True
    sched.save(update_fields=["user_disabled"])

    assert fire_tick(str(sched.id)) is False

    sched.refresh_from_db()
    assert sched.used == 0
    assert sched.enabled is False
    assert sched.disarm_reason == TickerDisarmReason.USER_DISABLED
    assert AgentRun.objects.filter(work_item=issue, parent_run__isnull=False).count() == 0
