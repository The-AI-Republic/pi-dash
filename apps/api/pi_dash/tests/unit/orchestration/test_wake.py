# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Wake dependents on blocker completion + pause cadence while the agent
waits (PDASHOSS01-198): ``orchestration.wake``, the ``fire_tick`` pre-claim
skip / forced wake, and the ``Issue`` post-save hook."""

from __future__ import annotations

from datetime import timedelta
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.bgtasks.agent_ticker import fire_tick
from pi_dash.db.models import Issue, IssueActivity, IssueComment, IssueRelation, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.orchestration import scheduling, wake
from pi_dash.orchestration.workpad import get_agent_system_user, parse_waiting_on, set_workpad
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


# ---------------------------------------------------------------------------
# fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(name="Web", identifier="WEB", workspace=workspace, created_by=create_user)


@pytest.fixture
def states(project, create_user):
    with impersonate(create_user):
        return {
            "backlog": State.objects.create(name="Backlog", project=project, group="backlog"),
            "todo": State.objects.create(name="Todo", project=project, group="unstarted"),
            "in_progress": State.objects.create(name="In Progress", project=project, group="started"),
            "in_review": State.objects.create(name="In Review", project=project, group="review"),
            "done": State.objects.create(name="Done", project=project, group="completed"),
            "cancelled": State.objects.create(name="Cancelled", project=project, group="cancelled"),
        }


@pytest.fixture
def make_issue(workspace, project, states, create_user):
    """Create in Todo, then move with a queryset update so no transition
    hook dispatches runs behind the test's back."""

    def _make(name, state="todo"):
        with impersonate(create_user):
            issue = Issue.objects.create(
                name=name, workspace=workspace, project=project, state=states["todo"], created_by=create_user
            )
        Issue.all_objects.filter(pk=issue.pk).update(state=states[state])
        issue.refresh_from_db()
        return issue

    return _make


@pytest.fixture
def runner(db, workspace, project, create_user):
    from pi_dash.runner.models import Pod, Runner, RunnerStatus

    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=Pod.default_for_project(project),
        name="agentA",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def stub_drain(monkeypatch):
    from pi_dash.runner.services import matcher

    monkeypatch.setattr(matcher, "drain_pod_by_id", mock.Mock())
    monkeypatch.setattr("django.db.transaction.on_commit", lambda fn, **kw: fn())


@pytest.fixture
def run_wake_inline(monkeypatch):
    """Run the Celery wake task synchronously when the signal enqueues it."""
    calls = []

    def _delay(blocker_id):
        calls.append(blocker_id)
        return wake.wake_dependents(blocker_id)

    monkeypatch.setattr(wake.wake_dependents, "delay", _delay)
    return calls


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _block(dependent, blocker):
    return IssueRelation.objects.create(
        issue=dependent,
        related_issue=blocker,
        relation_type="blocked_by",
        project=dependent.project,
        workspace=dependent.workspace,
    )


def _finished_run(issue, runner, status="waiting_on_external", ended_ago=timedelta(minutes=5)):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=runner.owner,
        pod=runner.pod,
        work_item=issue,
        runner=runner,
        thread_id="sess_xyz",
        status=AgentRunStatus.COMPLETED,
        prompt="prior work",
        done_payload={"status": status},
        started_at=timezone.now() - ended_ago - timedelta(minutes=1),
        ended_at=timezone.now() - ended_ago,
    )


def _wait_on(issue, *blockers):
    set_workpad(issue, "### Notes\n\n- Waiting on: " + ", ".join(_ident(b) for b in blockers) + "\n")
    issue.refresh_from_db()


def _armed(issue, *, due=True):
    ticker = scheduling.arm_ticker(issue)
    ticker.next_run_at = timezone.now() + (timedelta(seconds=-1) if due else timedelta(hours=3))
    ticker.save(update_fields=["next_run_at", "updated_at"])
    return ticker


@pytest.fixture
def waiting(make_issue, runner):
    """B (In Progress) blocked by open A, last run waited with ``Waiting on: A``."""
    blocker = make_issue("Model", "in_progress")
    dependent = make_issue("Handler", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner)
    _wait_on(dependent, blocker)
    return dependent, blocker


# ---------------------------------------------------------------------------
# marker parser
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "body,expected",
    [
        ("Waiting on: PDASHOSS01-123, PDASHOSS01-124", ["PDASHOSS01-123", "PDASHOSS01-124"]),
        ("- **Waiting on**: `web-1` and WEB-2", ["WEB-1", "WEB-2"]),
        ("  - **Waiting on:** WEB-3", ["WEB-3"]),
        ("> waiting on: WEB-4", ["WEB-4"]),
        ("Waiting on: WEB-1\n\nWaiting on: WEB-1, WEB-02", ["WEB-1", "WEB-2"]),
        ("- Waiting on: none", []),
        ("- Waiting on: <`none`, or the open blocker IDs you chose to wait for, comma-separated>", []),
        ("```md\nWaiting on: WEB-9\n```", []),
        ("Not waiting on: WEB-1", []),
        ("Waiting on WEB-1", []),
        ("", []),
        (None, []),
    ],
)
def test_parse_waiting_on(body, expected):
    assert parse_waiting_on(body) == expected


# ---------------------------------------------------------------------------
# waiting_pause
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_pause_holds_while_listed_blocker_is_open(waiting):
    dependent, blocker = waiting
    assert wake.waiting_pause(dependent) == [_ident(blocker)]


@pytest.mark.unit
def test_legacy_noop_status_also_pauses(make_issue, runner):
    blocker, dependent = make_issue("A", "todo"), make_issue("B", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner, status="noop")
    _wait_on(dependent, blocker)
    assert wake.waiting_pause(dependent) == [_ident(blocker)]


@pytest.mark.unit
@pytest.mark.parametrize("status", ["progressed", "done", "blocked", "waiting_on_human", None])
def test_no_pause_unless_last_run_waited(make_issue, runner, status):
    blocker, dependent = make_issue("A", "todo"), make_issue("B", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner, status=status)
    _wait_on(dependent, blocker)
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_no_pause_while_a_run_is_active(waiting, runner):
    dependent, _ = waiting
    run = _finished_run(dependent, runner)
    AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.RUNNING, ended_at=None)
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_no_pause_without_marker(waiting):
    dependent, _ = waiting
    set_workpad(dependent, "### Notes\n\n- Waiting on: none\n")
    dependent.refresh_from_db()
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_marker_naming_an_already_done_issue_does_not_pause(make_issue, runner):
    blocker, dependent = make_issue("A", "done"), make_issue("B", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner)
    _wait_on(dependent, blocker)
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_marker_with_one_closed_and_one_open_blocker_does_not_pause(make_issue, runner):
    a, c, dependent = make_issue("A", "done"), make_issue("C", "todo"), make_issue("B", "in_progress")
    _block(dependent, a)
    _block(dependent, c)
    _finished_run(dependent, runner)
    _wait_on(dependent, a, c)
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_marker_naming_an_unrelated_issue_does_not_pause(make_issue, runner):
    # Nothing would wake the dependent when an unrelated issue closes, so a
    # marker on it must not hold the clock.
    unrelated, dependent = make_issue("X", "todo"), make_issue("B", "in_progress")
    _finished_run(dependent, runner)
    _wait_on(dependent, unrelated)
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_marker_naming_an_unknown_identifier_does_not_pause(waiting):
    dependent, blocker = waiting
    set_workpad(dependent, f"Waiting on: {_ident(blocker)}, WEB-9999\n")
    dependent.refresh_from_db()
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_human_comment_after_the_waiting_run_ends_the_pause(waiting, create_user):
    dependent, _ = waiting
    IssueComment.objects.create(
        issue=dependent,
        project=dependent.project,
        workspace=dependent.workspace,
        actor=create_user,
        comment_html="<p>go ahead, the interface is settled</p>",
    )
    assert wake.waiting_pause(dependent) is None


@pytest.mark.unit
def test_agent_comment_does_not_end_the_pause(waiting, create_user):
    dependent, blocker = waiting
    # The agent's own "waiting on WEB-1" comment is posted with the runner
    # owner's token but spoken as the agent.
    IssueComment.objects.create(
        issue=dependent,
        project=dependent.project,
        workspace=dependent.workspace,
        actor=create_user,
        speaker_type=IssueComment.SpeakerType.AGENT,
        comment_html="<p>waiting on the model</p>",
    )
    IssueComment.objects.create(
        issue=dependent,
        project=dependent.project,
        workspace=dependent.workspace,
        actor=get_agent_system_user(),
        comment_html="<p>bot note</p>",
    )
    assert wake.waiting_pause(dependent) == [_ident(blocker)]


@pytest.mark.unit
def test_max_pause_elapsed_resumes(make_issue, runner):
    blocker, dependent = make_issue("A", "todo"), make_issue("B", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner, ended_ago=timedelta(days=7, minutes=1))
    _wait_on(dependent, blocker)
    assert wake.waiting_pause(dependent) is None
    # Within a longer project cap it still pauses.
    dependent.project.agent_wait_max_pause_seconds = 14 * 24 * 3600
    dependent.project.save(update_fields=["agent_wait_max_pause_seconds"])
    dependent.refresh_from_db()
    assert wake.waiting_pause(dependent) == [_ident(blocker)]


@pytest.mark.unit
def test_max_pause_zero_turns_the_pause_off(waiting):
    dependent, _ = waiting
    dependent.project.agent_wait_max_pause_seconds = 0
    dependent.project.save(update_fields=["agent_wait_max_pause_seconds"])
    dependent.refresh_from_db()
    assert wake.waiting_pause(dependent) is None


# ---------------------------------------------------------------------------
# fire_tick — cadence skip and forced wake
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_cadence_tick_skipped_while_waiting_without_spending_budget(seeded, waiting):
    dependent, _ = waiting
    ticker = _armed(dependent)
    runs_before = AgentRun.objects.filter(work_item=dependent).count()

    assert fire_tick(str(ticker.id)) is False

    ticker.refresh_from_db()
    assert ticker.used == 0
    assert ticker.enabled is True
    # Re-timed one interval out so the scanner doesn't re-fire it every minute.
    assert ticker.next_run_at > timezone.now() + timedelta(hours=2)
    assert AgentRun.objects.filter(work_item=dependent).count() == runs_before


@pytest.mark.unit
def test_cadence_tick_fires_once_max_pause_elapsed(seeded, make_issue, runner):
    blocker, dependent = make_issue("A", "todo"), make_issue("B", "in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner, ended_ago=timedelta(days=8))
    _wait_on(dependent, blocker)
    ticker = _armed(dependent)

    assert fire_tick(str(ticker.id)) is True
    ticker.refresh_from_db()
    assert ticker.used == 1


@pytest.mark.unit
def test_cadence_tick_fires_after_a_human_comment(seeded, waiting, create_user):
    dependent, _ = waiting
    IssueComment.objects.create(
        issue=dependent,
        project=dependent.project,
        workspace=dependent.workspace,
        actor=create_user,
        comment_html="<p>proceed</p>",
    )
    ticker = _armed(dependent)
    assert fire_tick(str(ticker.id)) is True


@pytest.mark.unit
def test_pending_entry_is_not_paused(seeded, waiting, create_user):
    # A human moved the issue / asked for a run while it was busy: the owed
    # entry run fires even with the marker in place.
    dependent, _ = waiting
    ticker = _armed(dependent)
    ticker.pending_entry = True
    ticker.pending_entry_free = True
    ticker.pending_entry_actor = create_user
    ticker.pending_entry_trigger = "state_transition"
    ticker.save()
    assert fire_tick(str(ticker.id)) is True
    ticker.refresh_from_db()
    assert ticker.used == 0


@pytest.mark.unit
def test_forced_wake_fires_before_cadence_and_counts(seeded, waiting):
    dependent, _ = waiting
    ticker = _armed(dependent, due=False)

    assert fire_tick(str(ticker.id), trigger=scheduling.TRIGGER_BLOCKER_COMPLETED) is True

    ticker.refresh_from_db()
    assert ticker.used == 1
    run = AgentRun.objects.filter(work_item=dependent).order_by("-created_at").first()
    assert run.trigger == "blocker_completed"
    assert "blocked by was just completed or cancelled" in run.prompt


@pytest.mark.unit
def test_forced_wake_respects_active_run(seeded, waiting, runner):
    dependent, _ = waiting
    ticker = _armed(dependent, due=False)
    run = _finished_run(dependent, runner)
    AgentRun.objects.filter(pk=run.pk).update(status=AgentRunStatus.RUNNING, ended_at=None)

    assert fire_tick(str(ticker.id), trigger=scheduling.TRIGGER_BLOCKER_COMPLETED) is False
    ticker.refresh_from_db()
    assert ticker.used == 0


# ---------------------------------------------------------------------------
# wake_dependents
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_wake_records_activity_on_the_dependent(seeded, waiting, states):
    dependent, blocker = waiting
    _armed(dependent, due=False)
    Issue.all_objects.filter(pk=blocker.pk).update(state=states["done"])

    assert wake.wake_dependents(str(blocker.pk)) == 1

    activity = IssueActivity.objects.get(issue=dependent, field=wake.WAKE_ACTIVITY_FIELD)
    assert activity.comment == f"Woken: {_ident(blocker)} completed"
    assert activity.new_value == _ident(blocker)
    assert activity.actor == get_agent_system_user()


@pytest.mark.unit
@pytest.mark.parametrize("state", ["backlog", "todo"])
def test_wake_leaves_backlog_and_todo_dependents_alone(seeded, make_issue, runner, states, state):
    blocker, dependent = make_issue("A", "done"), make_issue("B", state)
    _block(dependent, blocker)
    _finished_run(dependent, runner)

    assert wake.wake_dependents(str(blocker.pk)) == 0

    dependent.refresh_from_db()
    assert dependent.state_id == states[state].id
    assert not AgentRun.objects.filter(work_item=dependent, trigger="blocker_completed").exists()
    assert not IssueActivity.objects.filter(issue=dependent, field=wake.WAKE_ACTIVITY_FIELD).exists()


@pytest.mark.unit
def test_wake_skips_a_stopped_clock(seeded, waiting, states):
    dependent, blocker = waiting
    ticker = _armed(dependent, due=False)
    IssueAgentTicker.objects.filter(pk=ticker.pk).update(enabled=False)
    Issue.all_objects.filter(pk=blocker.pk).update(state=states["done"])

    assert wake.wake_dependents(str(blocker.pk)) == 0


@pytest.mark.unit
def test_wake_is_a_noop_if_blocker_reopened_before_task_ran(seeded, waiting):
    dependent, blocker = waiting
    _armed(dependent, due=False)
    assert wake.wake_dependents(str(blocker.pk)) == 0


# ---------------------------------------------------------------------------
# integration: signal -> task -> fire_tick
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_blocker_done_wakes_dependent_each_time_through_the_signal(seeded, make_issue, runner, states, run_wake_inline):
    a, c = make_issue("Model", "in_progress"), make_issue("Serializer", "in_progress")
    dependent = make_issue("Handler", "in_progress")
    _block(dependent, a)
    _block(dependent, c)
    _finished_run(dependent, runner)
    _wait_on(dependent, a, c)
    ticker = _armed(dependent, due=False)

    # While both are open the cadence tick is paused.
    assert wake.waiting_pause(dependent) == [_ident(a), _ident(c)]

    a.state = states["done"]
    a.save()

    assert run_wake_inline == [str(a.pk)]
    woken = AgentRun.objects.filter(work_item=dependent, trigger="blocker_completed")
    assert woken.count() == 1
    ticker.refresh_from_db()
    assert ticker.used == 1

    # The woken run decides C is still worth waiting for and says so.
    AgentRun.objects.filter(pk=woken.get().pk).update(
        status=AgentRunStatus.COMPLETED,
        done_payload={"status": "waiting_on_external"},
        ended_at=timezone.now(),
    )
    _wait_on(dependent, c)
    assert wake.waiting_pause(dependent) == [_ident(c)]

    c.state = states["done"]
    c.save()

    assert AgentRun.objects.filter(work_item=dependent, trigger="blocker_completed").count() == 2
    assert IssueActivity.objects.filter(issue=dependent, field=wake.WAKE_ACTIVITY_FIELD).count() == 2


@pytest.mark.unit
def test_resaving_a_closed_blocker_does_not_re_fire(seeded, waiting, states, run_wake_inline):
    dependent, blocker = waiting
    _armed(dependent, due=False)
    blocker.state = states["done"]
    blocker.save()
    assert len(run_wake_inline) == 1

    blocker.refresh_from_db()
    blocker.name = "Model (renamed)"
    blocker.save()
    blocker.state = states["cancelled"]
    blocker.save()

    assert len(run_wake_inline) == 1
    assert AgentRun.objects.filter(work_item=dependent, trigger="blocker_completed").count() == 1


@pytest.mark.unit
def test_blocker_moving_between_open_states_does_not_wake(seeded, waiting, states, run_wake_inline):
    dependent, blocker = waiting
    _armed(dependent, due=False)
    blocker.state = states["in_review"]
    blocker.save()
    assert run_wake_inline == []
    assert wake.waiting_pause(dependent) is not None
