# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``scheduling.reconcile`` — one row per event in the §4.0 event table.

See ``.ai_design/ticking_relevance/design.md`` §4.0 (event table), §4.5
(entry-run queue), §5 (one pool), §7 (outcome guard), §13 (this list).
"""

from __future__ import annotations

from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import (
    IssueAgentTicker,
    TickerDisarmReason,
)
from pi_dash.orchestration import scheduling, service
from pi_dash.orchestration.scheduling import TickerEvent
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
            agent_default_max_ticks=10,
            agent_retick_grant=3,
        )


@pytest.fixture
def states(project, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(name="Todo", project=project, group="unstarted"),
            "in_progress": State.objects.create(name="In Progress", project=project, group="started"),
            "in_review": State.objects.create(name="In Review", project=project, group="review"),
            "in_test": State.objects.create(name="In Test", project=project, group="test"),
            "paused": State.objects.create(name="Paused", project=project, group="backlog"),
            "done": State.objects.create(name="Done", project=project, group="completed"),
        }


@pytest.fixture
def issue(workspace, project, states, create_user):
    with impersonate(create_user):
        return Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )


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


def _move(issue, states, key):
    """Change state without firing the post_save signal."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states[key])
    issue.refresh_from_db()
    return issue


def _run(issue, runner, *, status=AgentRunStatus.RUNNING, phase_kind="coding-task", done_payload=None):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=runner.owner,
        pod=runner.pod,
        work_item=issue,
        runner=runner,
        status=status,
        phase_kind=phase_kind,
        done_payload=done_payload,
        prompt="x",
        started_at=timezone.now(),
    )


def _ticker(issue, **kw):
    return IssueAgentTicker.objects.create(issue=issue, **kw)


# ---------------------------------------------------------------------------
# Entering the bucket / moving between rooms
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_human_entry_dispatches_now_and_arms_the_clock(seeded, issue, states):
    _move(issue, states, "in_progress")
    decision = scheduling.reconcile(issue, TickerEvent.entered_bucket())
    assert decision.dispatch_now is True
    assert decision.queued is False
    t = decision.ticker
    assert t.enabled is True
    assert t.used == 0
    assert t.next_run_at > timezone.now()
    assert t.pending_entry is False


@pytest.mark.unit
def test_human_entry_with_active_run_queues_a_free_entry(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _run(issue, runner_for_workspace)
    decision = scheduling.reconcile(issue, TickerEvent.entered_bucket())
    assert decision.dispatch_now is False
    assert decision.queued is True
    t = decision.ticker
    assert t.enabled is True
    assert t.pending_entry is True
    assert t.pending_entry_free is True
    assert t.next_run_at <= timezone.now()


@pytest.mark.unit
def test_human_move_into_a_spent_pool_still_fires_but_keeps_the_clock_stopped(
    seeded, issue, states
):
    _move(issue, states, "in_progress")
    _ticker(issue, used=10)
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage())
    assert decision.dispatch_now is True
    t = decision.ticker
    assert t.used == 10
    assert t.enabled is False
    # POOL_SPENT, not CAP_HIT: the human's free run must not end in an
    # auto-Pause that hides the Re-tick button.
    assert t.disarm_reason == TickerDisarmReason.POOL_SPENT


@pytest.mark.unit
def test_agent_move_queues_a_counting_entry(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_review")
    run = _run(issue, runner_for_workspace)
    _ticker(issue, used=3)
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage(moved_by_run=run))
    assert decision.queued is True
    assert decision.dispatch_now is False
    t = decision.ticker
    assert t.pending_entry is True
    assert t.pending_entry_free is False
    assert t.enabled is True
    assert t.next_run_at <= timezone.now()
    assert t.used == 3  # fire_tick, not reconcile, spends


@pytest.mark.unit
def test_agent_move_with_pool_spent_parks_the_issue(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    run = _run(issue, runner_for_workspace)
    _ticker(issue, used=10)
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage(moved_by_run=run))
    assert decision.parked is True
    assert decision.queued is False
    assert decision.dispatch_now is False
    t = decision.ticker
    assert t.enabled is False
    assert t.disarm_reason == TickerDisarmReason.POOL_SPENT
    assert t.pending_entry is False


@pytest.mark.unit
def test_move_never_resets_used_and_re_reads_the_interval(seeded, issue, states, project):
    project.agent_default_interval_seconds = 36000
    project.agent_review_default_interval_seconds = 3600
    project.save(update_fields=["agent_default_interval_seconds", "agent_review_default_interval_seconds"])
    _move(issue, states, "in_progress")
    t = _ticker(issue, used=6)
    _move(issue, states, "in_review")
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage())
    t.refresh_from_db()
    assert t.used == 6
    assert decision.ticker.pk == t.pk
    # Re-timed on the review interval (1 h, plus ≤10% jitter), not 10 h.
    delta = (t.next_run_at - timezone.now()).total_seconds()
    assert 3500 < delta < 4000


@pytest.mark.unit
def test_move_remembers_the_implementation_parent(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    impl = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED)
    _move(issue, states, "in_review")
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage(resume_parent=impl))
    assert decision.ticker.resume_parent_run_id == impl.id


@pytest.mark.unit
def test_user_disabled_blocks_arming_but_not_a_free_run(seeded, issue, states):
    _move(issue, states, "in_progress")
    _ticker(issue, user_disabled=True)
    decision = scheduling.reconcile(issue, TickerEvent.entered_bucket())
    assert decision.dispatch_now is True
    assert decision.ticker.enabled is False
    assert decision.ticker.disarm_reason == TickerDisarmReason.USER_DISABLED


# ---------------------------------------------------------------------------
# Leaving the bucket
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_left_bucket_goes_dormant_and_keeps_the_pool(seeded, issue, states):
    _move(issue, states, "in_progress")
    _ticker(issue, used=4, granted=3, enabled=True, next_run_at=timezone.now(), pending_entry=True)
    _move(issue, states, "done")
    decision = scheduling.reconcile(issue, TickerEvent.left_bucket())
    t = decision.ticker
    assert t.enabled is False
    assert t.disarm_reason == TickerDisarmReason.LEFT_TICKING_STATE
    assert t.next_run_at is None
    assert t.pending_entry is False
    assert (t.used, t.granted) == (4, 3)


@pytest.mark.unit
def test_left_bucket_without_ticker_is_a_noop(seeded, issue, states):
    _move(issue, states, "done")
    decision = scheduling.reconcile(issue, TickerEvent.left_bucket())
    assert decision.ticker is None
    assert decision.reason == "no-ticker"


# ---------------------------------------------------------------------------
# A run ended (outcome + the §7 guard)
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize("outcome", ["done", "blocked", "waiting_on_human"])
def test_stopping_outcome_stops_the_clock_in_place(seeded, issue, states, runner_for_workspace, outcome):
    _move(issue, states, "in_review")
    _ticker(issue, enabled=True, next_run_at=timezone.now())
    run = _run(
        issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="review",
        done_payload={"status": outcome},
    )
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == f"{outcome}:stopped"
    assert decision.ticker.enabled is False
    assert decision.ticker.disarm_reason == TickerDisarmReason.TERMINAL_SIGNAL


@pytest.mark.unit
@pytest.mark.parametrize("outcome", ["progressed", "waiting_on_external"])
def test_continuing_outcome_keeps_ticking(seeded, issue, states, runner_for_workspace, outcome):
    _move(issue, states, "in_progress")
    _ticker(issue, enabled=True, next_run_at=None)
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, done_payload={"status": outcome})
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == f"{outcome}:keep-ticking"
    assert decision.ticker.enabled is True
    assert decision.ticker.next_run_at > timezone.now()


@pytest.mark.unit
def test_done_after_a_forward_move_leaves_the_next_stage_clock_alone(
    seeded, issue, states, runner_for_workspace
):
    """The §7 guard: the run was rendered for In Progress, moved the issue to
    In Review (the clock now holds the queued review entry), then yielded
    ``done``. The review clock must survive."""
    _move(issue, states, "in_progress")
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="coding-task",
               done_payload={"status": "done"})
    _move(issue, states, "in_review")
    scheduling.reconcile(issue, TickerEvent.moved_stage(moved_by_run=run))
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.pending_entry is True

    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == "stage-moved-on"
    t.refresh_from_db()
    assert t.enabled is True
    assert t.pending_entry is True


@pytest.mark.unit
def test_run_without_a_yield_uses_the_per_kind_default(seeded, issue, states, runner_for_workspace):
    # coding-task → progressed (keep ticking); review/test → done (stop).
    _move(issue, states, "in_progress")
    t = _ticker(issue, enabled=True, next_run_at=timezone.now())
    impl = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="coding-task",
                done_payload={"conclusion": "success"})
    assert scheduling.reconcile(issue, TickerEvent.run_ended(impl)).reason == "progressed:keep-ticking"
    t.refresh_from_db()
    assert t.enabled is True

    _move(issue, states, "in_review")
    review = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="review",
                  done_payload={"conclusion": "success"})
    assert scheduling.reconcile(issue, TickerEvent.run_ended(review)).reason == "done:stopped"
    t.refresh_from_db()
    assert t.enabled is False


@pytest.mark.unit
def test_run_ended_does_not_override_a_pending_entry(seeded, issue, states, runner_for_workspace):
    """A human's queued follow-up outranks the finished run's ``done``."""
    _move(issue, states, "in_review")
    _ticker(issue, enabled=True, next_run_at=timezone.now(), pending_entry=True, pending_entry_free=True)
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="review",
               done_payload={"status": "done"})
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == "done:pending-entry-kept"
    assert decision.ticker.enabled is True
    assert decision.ticker.pending_entry is True


@pytest.mark.unit
def test_run_ended_preserves_a_prior_cap_hit(seeded, issue, states, runner_for_workspace):
    """A ``done`` must not overwrite ``cap_hit`` — that reason gates the
    deferred auto-pause."""
    _move(issue, states, "in_progress")
    _ticker(issue, used=10, enabled=False, disarm_reason=TickerDisarmReason.CAP_HIT)
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, done_payload={"status": "done"})
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == "done:already-stopped"
    assert decision.ticker.disarm_reason == TickerDisarmReason.CAP_HIT


@pytest.mark.unit
def test_run_ended_outside_the_bucket_is_ignored(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "done")
    _ticker(issue, enabled=False)
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, done_payload={"status": "done"})
    assert scheduling.reconcile(issue, TickerEvent.run_ended(run)).reason == "not-in-bucket"


# ---------------------------------------------------------------------------
# Human levers
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_human_run_requested_retimes_when_budget_remains(seeded, issue, states):
    _move(issue, states, "in_progress")
    _ticker(issue, used=4, enabled=False, disarm_reason=TickerDisarmReason.TERMINAL_SIGNAL)
    decision = scheduling.reconcile(issue, TickerEvent.human_run_requested())
    assert decision.dispatch_now is True
    t = decision.ticker
    assert t.used == 4
    assert t.enabled is True
    assert t.disarm_reason == TickerDisarmReason.NONE


@pytest.mark.unit
def test_human_run_requested_with_active_run_queues_free(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _run(issue, runner_for_workspace)
    _ticker(issue, used=4)
    decision = scheduling.reconcile(issue, TickerEvent.human_run_requested())
    assert decision.queued is True
    assert decision.ticker.pending_entry_free is True


@pytest.mark.unit
def test_retick_grants_and_dispatches(seeded, issue, states):
    _move(issue, states, "in_progress")
    _ticker(issue, used=10, enabled=False, disarm_reason=TickerDisarmReason.CAP_HIT)
    decision = scheduling.reconcile(issue, TickerEvent.retick())
    assert decision.granted is True
    assert decision.dispatch_now is True
    t = decision.ticker
    assert t.granted == 3
    assert t.effective_max_ticks() == 13
    assert t.enabled is True


@pytest.mark.unit
def test_retick_guards(seeded, issue, states):
    # Not in the bucket.
    _ticker(issue, used=10)
    assert scheduling.reconcile(issue, TickerEvent.retick()).reason == "not_ticking_state"
    # Budget not exhausted.
    _move(issue, states, "in_progress")
    IssueAgentTicker.objects.filter(issue=issue).update(used=2)
    assert scheduling.reconcile(issue, TickerEvent.retick()).reason == "budget_not_exhausted"


@pytest.mark.unit
def test_retick_with_active_run_queues_a_free_entry(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _run(issue, runner_for_workspace)
    _ticker(issue, used=10, enabled=False, disarm_reason=TickerDisarmReason.CAP_HIT)
    decision = scheduling.reconcile(issue, TickerEvent.retick())
    assert decision.granted is True
    assert decision.queued is True
    assert decision.ticker.pending_entry_free is True


# ---------------------------------------------------------------------------
# The transition handler end to end (§4.0 entry cases, §5.6 agent vs human)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_agent_forward_move_queues_the_next_stage_entry(seeded, issue, states, runner_for_workspace):
    """Today's 8 h review delay: an implementation run moves the issue to In
    Review from inside itself. The review entry must be queued *now*, not
    one interval later."""
    _move(issue, states, "in_progress")
    run = _run(issue, runner_for_workspace)
    _ticker(issue, used=2, enabled=True, next_run_at=timezone.now())
    _move(issue, states, "in_review")
    outcome = service.handle_issue_state_transition(
        issue=issue, from_state=states["in_progress"], to_state=states["in_review"], moved_by_run=run
    )
    assert outcome.reason == "entry-queued"
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.pending_entry is True
    assert t.pending_entry_free is False
    assert t.next_run_at <= timezone.now()
    assert t.used == 2
    assert t.resume_parent_run_id == run.id


@pytest.mark.unit
def test_agent_backward_move_with_pool_spent_parks(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_review")
    run = _run(issue, runner_for_workspace, phase_kind="review")
    _ticker(issue, used=10)
    _move(issue, states, "in_progress")
    outcome = service.handle_issue_state_transition(
        issue=issue, from_state=states["in_review"], to_state=states["in_progress"], moved_by_run=run
    )
    assert outcome.reason == "pool-spent"
    assert outcome.created_run is None
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.enabled is False
    assert t.disarm_reason == TickerDisarmReason.POOL_SPENT
    assert AgentRun.objects.filter(work_item=issue).count() == 1
    # And when the moving run ends, the parked issue is NOT auto-paused —
    # the §5.4 comment told the human to Re-tick it from here.
    AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.COMPLETED)
    run.refresh_from_db()
    assert scheduling.maybe_apply_deferred_pause(run) is False
    issue.refresh_from_db()
    assert issue.state == states["in_progress"]


@pytest.mark.unit
def test_human_move_into_a_spent_pool_fires_one_free_run(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _ticker(issue, used=10)
    with mock.patch.object(service, "_create_and_dispatch_run", wraps=service._create_and_dispatch_run) as spy:
        outcome = service.handle_issue_state_transition(
            issue=issue, from_state=states["todo"], to_state=states["in_progress"]
        )
    assert spy.call_count == 1
    assert outcome.reason == "created"
    assert outcome.created_run.trigger == "state_transition"
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.used == 10
    assert t.enabled is False


@pytest.mark.unit
def test_direct_entry_into_review_from_backlog_is_a_free_review_run(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_review")
    outcome = service.handle_issue_state_transition(
        issue=issue, from_state=states["todo"], to_state=states["in_review"]
    )
    assert outcome.reason == "created"
    run = outcome.created_run
    assert run.phase_kind == "review"
    assert run.parent_run_id is None
    assert "no prior run output above and no workpad" in run.prompt
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.used == 0
    assert t.enabled is True


@pytest.mark.unit
def test_leaving_the_bucket_makes_the_clock_dormant(seeded, issue, states):
    _move(issue, states, "in_progress")
    _ticker(issue, used=3, enabled=True, next_run_at=timezone.now())
    _move(issue, states, "done")
    outcome = service.handle_issue_state_transition(
        issue=issue, from_state=states["in_progress"], to_state=states["done"]
    )
    assert outcome.reason == "not-a-trigger-state"
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.enabled is False
    assert t.next_run_at is None
    assert t.used == 3


@pytest.mark.unit
def test_dispatch_deferred_to_caller_only_retimes(seeded, issue, states):
    _move(issue, states, "in_progress")
    outcome = service.handle_issue_state_transition(
        issue=issue, from_state=states["paused"], to_state=states["in_progress"], dispatch_immediate=False
    )
    assert outcome.reason == "dispatch-deferred-to-caller"
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.enabled is True
    assert t.pending_entry is False
    assert AgentRun.objects.filter(work_item=issue).count() == 0


# ---------------------------------------------------------------------------
# Deferred pause interplay
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_deferred_pause_skips_when_an_entry_is_pending(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _ticker(issue, used=10, enabled=False, disarm_reason=TickerDisarmReason.CAP_HIT,
            pending_entry=True, pending_entry_free=True, next_run_at=timezone.now())
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED)
    assert scheduling.maybe_apply_deferred_pause(run) is False
    issue.refresh_from_db()
    assert issue.state == states["in_progress"]


@pytest.mark.unit
def test_outcome_normalization():
    n = scheduling.normalize_outcome
    assert n("done") == "done"
    assert n(" DONE ") == "done"
    assert n("completed") == "done"
    # "nothing changed" follows the kind: an implementation run keeps
    # ticking (CI may still be running); a satisfied review / test stops.
    assert n("noop") == "progressed"
    assert n("noop", "coding-task") == "progressed"
    assert n("noop", "review") == "done"
    assert n("noop", "test") == "done"
    assert n("paused") == "waiting_on_human"
    assert n("blocked") == "blocked"
    assert n("progressed") == "progressed"
    assert n("success") is None
    assert n(None) is None
    assert n(3) is None


# ---------------------------------------------------------------------------
# Review fixes: failed runs, paused runs, queued actors, switched-off clocks
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize("status", [AgentRunStatus.FAILED, AgentRunStatus.CANCELLED, AgentRunStatus.REFUSED])
def test_a_crashed_review_run_keeps_the_clock_ticking(seeded, issue, states, runner_for_workspace, status):
    """A run that failed said nothing about the stage: the next tick
    retries, instead of a per-kind 'done' stranding the issue with no
    Re-tick button."""
    _move(issue, states, "in_review")
    _ticker(issue, used=3, enabled=True, next_run_at=timezone.now())
    run = _run(issue, runner_for_workspace, status=status, phase_kind="review", done_payload=None)
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == "progressed:keep-ticking"
    assert decision.ticker.enabled is True


@pytest.mark.unit
def test_a_paused_run_without_a_yield_waits_on_the_human(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _ticker(issue, enabled=True, next_run_at=timezone.now())
    run = _run(
        issue, runner_for_workspace, status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        done_payload={"autonomy": {"question_for_human": "which DB?"}},
    )
    decision = scheduling.reconcile(issue, TickerEvent.run_ended(run))
    assert decision.reason == "waiting_on_human:stopped"


@pytest.mark.unit
def test_a_yield_survives_a_crash(seeded, issue, states, runner_for_workspace):
    """The agent yielded, then the process died: honour the yield."""
    _move(issue, states, "in_review")
    _ticker(issue, enabled=True, next_run_at=timezone.now())
    run = _run(
        issue, runner_for_workspace, status=AgentRunStatus.FAILED, phase_kind="review",
        done_payload={"status": "done", "yielded_at": "2026-09-13T00:00:00Z"},
    )
    assert scheduling.reconcile(issue, TickerEvent.run_ended(run)).reason == "done:stopped"


@pytest.mark.unit
def test_queued_human_entry_remembers_who_asked(seeded, issue, states, runner_for_workspace, create_user):
    _move(issue, states, "in_progress")
    _run(issue, runner_for_workspace)
    _ticker(issue, used=4)
    decision = scheduling.reconcile(
        issue, TickerEvent.human_run_requested(actor=create_user, trigger="comment_and_run")
    )
    assert decision.queued is True
    t = decision.ticker
    assert t.pending_entry_actor_id == create_user.id
    assert t.pending_entry_trigger == "comment_and_run"


@pytest.mark.unit
def test_agent_queued_entry_carries_no_actor(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_review")
    run = _run(issue, runner_for_workspace)
    _ticker(issue, used=3)
    decision = scheduling.reconcile(issue, TickerEvent.moved_stage(moved_by_run=run))
    t = decision.ticker
    assert t.pending_entry_actor_id is None
    assert t.pending_entry_trigger == ""


@pytest.mark.unit
def test_human_lever_on_a_switched_off_clock_queues_but_leaves_it_off_afterwards(
    seeded, issue, states, runner_for_workspace, create_user
):
    """A human asked for *this* run, so it fires even though automatic
    ticking is off; ``fire_tick`` then re-applies the switch so no timer
    tick follows."""
    from pi_dash.bgtasks.agent_ticker import fire_tick

    _move(issue, states, "in_progress")
    active = _run(issue, runner_for_workspace)
    _ticker(issue, used=2, user_disabled=True, enabled=False, disarm_reason=TickerDisarmReason.USER_DISABLED)
    decision = scheduling.reconcile(issue, TickerEvent.human_run_requested(actor=create_user))
    assert decision.queued is True
    assert decision.ticker.enabled is True  # the scanner must see it

    AgentRun.objects.filter(pk=active.pk).update(status=AgentRunStatus.COMPLETED)
    with mock.patch("pi_dash.orchestration.scheduling.dispatch_continuation_run") as dispatch:
        dispatch.return_value = mock.Mock(pk=uuid_for_test())
        assert fire_tick(str(decision.ticker.id)) is True
    t = IssueAgentTicker.objects.get(issue=issue)
    assert t.used == 2
    assert t.enabled is False
    assert t.disarm_reason == TickerDisarmReason.USER_DISABLED
    assert t.pending_entry is False


def uuid_for_test():
    import uuid

    return uuid.uuid4()


@pytest.mark.unit
def test_cloud_noop_on_an_in_progress_run_keeps_ticking(seeded, issue, states, runner_for_workspace):
    _move(issue, states, "in_progress")
    _ticker(issue, enabled=True, next_run_at=timezone.now())
    run = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="coding-task",
               done_payload={"status": "noop"})
    assert scheduling.reconcile(issue, TickerEvent.run_ended(run)).reason == "progressed:keep-ticking"


@pytest.mark.unit
def test_retick_is_honoured_from_paused(seeded, issue, states):
    _move(issue, states, "paused")
    _ticker(issue, used=10, enabled=False, disarm_reason=TickerDisarmReason.CAP_HIT)
    decision = scheduling.reconcile(issue, TickerEvent.retick())
    assert decision.granted is True
    assert decision.reason == "granted-from-paused"
    assert decision.ticker.granted == 3


@pytest.mark.unit
def test_queued_entry_into_test_starts_a_fresh_session(seeded, issue, states, runner_for_workspace, create_user):
    """A queued entry is parented like a transition dispatch: review → test
    is a fresh session, so the test prompt does not present the review
    run's yield as the 'authoritative implementation output'."""
    from pi_dash.bgtasks.agent_ticker import fire_tick

    _move(issue, states, "in_progress")
    impl = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="coding-task",
                done_payload={"conclusion": "success", "result": "PR opened"})
    _move(issue, states, "in_review")
    review = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="review",
                  done_payload={"status": "done", "note": "approved", "yielded_at": "x"})
    _move(issue, states, "in_test")
    t = _ticker(issue, used=2, enabled=True, next_run_at=timezone.now(), pending_entry=True,
                resume_parent_run=impl)
    assert fire_tick(str(t.id)) is True
    test_run = AgentRun.objects.filter(work_item=issue).order_by("-created_at").first()
    assert test_run.phase_kind == "test"
    assert test_run.parent_run_id is None
    assert "PR opened" in test_run.prompt
    assert "approved" not in test_run.prompt.split("Latest implementation run output")[1][:400]
    assert review.id != test_run.id


@pytest.mark.unit
def test_queued_hand_back_parents_off_the_implementation_run(seeded, issue, states, runner_for_workspace):
    from pi_dash.bgtasks.agent_ticker import fire_tick

    _move(issue, states, "in_progress")
    impl = _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="coding-task")
    _move(issue, states, "in_review")
    _run(issue, runner_for_workspace, status=AgentRunStatus.COMPLETED, phase_kind="review",
         done_payload={"status": "done", "yielded_at": "x"})
    _move(issue, states, "in_progress")
    t = _ticker(issue, used=2, enabled=True, next_run_at=timezone.now(), pending_entry=True,
                resume_parent_run=impl)
    assert fire_tick(str(t.id)) is True
    back = AgentRun.objects.filter(work_item=issue).order_by("-created_at").first()
    assert back.phase_kind == "coding-task"
    assert back.parent_run_id == impl.id
