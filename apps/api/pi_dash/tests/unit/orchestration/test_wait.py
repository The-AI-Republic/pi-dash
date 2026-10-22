# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The agent-called wait replaces the platform-driven blocker pause (PDASHOSS01-204).

Two halves, and the second is as load-bearing as the first:

- ``scheduling.wait_ticker`` — the agent says it is waiting, and the tick its
  run is spending is bought back, bounded by one extra pool.
- What the platform *stopped* doing — no cadence skip inferred from workpad
  prose, and no run fired on a dependent when its blocker closes. Those are
  asserted as absences, because an absence is exactly what regressed here.
"""

from __future__ import annotations

from datetime import timedelta
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.bgtasks.agent_ticker import fire_tick, scan_due_tickers
from pi_dash.db.models import Issue, IssueActivity, IssueRelation, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker, TickerDisarmReason
from pi_dash.orchestration import scheduling
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
            "paused": State.objects.create(name="Paused", project=project, group="backlog"),
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


@pytest.fixture
def waiting_issue(seeded, make_issue, project):
    """An In Progress issue with an armed clock and a default (10) pool."""
    project.agent_default_max_ticks = 10
    project.save(update_fields=["agent_default_max_ticks"])
    issue = make_issue("B", state="in_progress")
    IssueAgentTicker.objects.filter(issue=issue).delete()
    IssueAgentTicker.objects.create(
        issue=issue,
        used=3,
        enabled=True,
        next_run_at=timezone.now() + timedelta(hours=3),
    )
    return issue


# ---------------------------------------------------------------------------
# wait_ticker — the budget
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_wait_raises_the_cap_by_one_so_the_run_costs_no_net_budget(waiting_issue):
    before = IssueAgentTicker.objects.get(issue=waiting_issue)
    cap_before, used_before = before.effective_max_ticks(), before.used

    result = scheduling.wait_ticker(waiting_issue)

    assert result["applied"] is True
    assert result["reason"] == scheduling.WAIT_GRANTED
    after = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert after.waited == 1
    assert after.effective_max_ticks() == cap_before + 1
    # The tick the ending run spent is refunded: ``used`` is untouched and
    # one more run than before is available.
    assert after.used == used_before
    assert after.remaining() == before.remaining() + 1


@pytest.mark.unit
def test_a_second_wait_in_the_same_run_adds_another_tick(waiting_issue):
    """The per-issue allowance is the only limit — one run may spend it."""
    first = scheduling.wait_ticker(waiting_issue)
    second = scheduling.wait_ticker(waiting_issue)

    assert [first["applied"], second["applied"]] == [True, True]
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert ticker.waited == 2
    assert ticker.effective_max_ticks() == 12


@pytest.mark.unit
def test_wait_past_the_allowance_is_refused_and_changes_nothing(waiting_issue):
    """The allowance is one extra pool: 10 waits on a 10-run pool, then no more."""
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    ticker.waited = 10
    ticker.save(update_fields=["waited"])
    cap_before = IssueAgentTicker.objects.get(issue=waiting_issue).effective_max_ticks()

    result = scheduling.wait_ticker(waiting_issue)

    assert result["applied"] is False
    assert result["reason"] == scheduling.WAIT_CAP_REACHED
    after = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert after.waited == 10
    assert after.effective_max_ticks() == cap_before
    # A refusal is silent on the activity feed — only real waits are logged.
    assert not IssueActivity.objects.filter(issue=waiting_issue, field=scheduling.WAIT_ACTIVITY_FIELD).exists()


@pytest.mark.unit
def test_the_ceiling_is_twenty_runs_on_the_default_pool(waiting_issue):
    """Ten runs of work plus ten of waiting, and not one more."""
    for _ in range(10):
        assert scheduling.wait_ticker(waiting_issue)["applied"] is True
    assert scheduling.wait_ticker(waiting_issue)["reason"] == scheduling.WAIT_CAP_REACHED

    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert ticker.waited == 10
    assert ticker.effective_max_ticks() == 20
    assert ticker.wait_allowance() == 0


@pytest.mark.unit
def test_wait_on_an_infinite_pool_is_a_noop_with_a_reason(waiting_issue, project):
    """There is no budget to buy back, so the command says so — as Re-tick does."""
    project.agent_default_max_ticks = -1
    project.save(update_fields=["agent_default_max_ticks"])

    result = scheduling.wait_ticker(waiting_issue)

    assert result["applied"] is False
    assert result["reason"] == scheduling.WAIT_INFINITE_POOL
    assert IssueAgentTicker.objects.get(issue=waiting_issue).waited == 0


@pytest.mark.unit
def test_wait_without_a_ticker_is_a_noop_with_a_reason(seeded, make_issue):
    issue = make_issue("never-ticked", state="todo")
    IssueAgentTicker.objects.filter(issue=issue).delete()

    result = scheduling.wait_ticker(issue)

    assert result["applied"] is False
    assert result["reason"] == scheduling.WAIT_NO_TICKER
    assert result["ticker"] is None


# ---------------------------------------------------------------------------
# wait_ticker — the clock
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_wait_rearms_a_clock_the_cap_just_stopped(waiting_issue):
    """The common case: the agent waits on the run that spent the last tick.

    ``fire_tick``'s claim disarms at CAP_HIT *before* the agent can call
    wait, so without the re-arm the refunded tick would be unspendable — and
    ``maybe_apply_deferred_pause`` would park the issue on that same reason.
    """
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    ticker.used = 10
    ticker.enabled = False
    ticker.disarm_reason = TickerDisarmReason.CAP_HIT
    ticker.next_run_at = None
    ticker.save(update_fields=["used", "enabled", "disarm_reason", "next_run_at"])

    assert scheduling.wait_ticker(waiting_issue)["applied"] is True

    after = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert after.enabled is True
    assert after.disarm_reason == TickerDisarmReason.NONE
    assert after.next_run_at is not None
    assert after.cap_reached() is False


@pytest.mark.unit
def test_wait_does_not_rearm_a_clock_stopped_for_another_reason(waiting_issue):
    """A wait buys budget, not a restart: only a spent pool is re-armed."""
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    ticker.enabled = False
    ticker.disarm_reason = TickerDisarmReason.LEFT_TICKING_STATE
    ticker.save(update_fields=["enabled", "disarm_reason"])

    assert scheduling.wait_ticker(waiting_issue)["applied"] is True

    after = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert after.enabled is False
    assert after.disarm_reason == TickerDisarmReason.LEFT_TICKING_STATE
    assert after.waited == 1


@pytest.mark.unit
def test_the_scanner_admits_an_issue_whose_pool_only_the_wait_reopened(waiting_issue):
    """``scan_due_tickers`` reproduces the cap in SQL; it must count waits.

    If it does not, the refunded tick is granted and then never fans out.
    """
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    ticker.used = 10
    ticker.next_run_at = timezone.now() - timedelta(minutes=1)
    ticker.save(update_fields=["used", "next_run_at"])

    with mock.patch.object(fire_tick, "delay") as delay:
        assert scan_due_tickers() == 0, "a spent pool must not be admitted"
        assert delay.call_count == 0

        scheduling.wait_ticker(waiting_issue)
        IssueAgentTicker.objects.filter(issue=waiting_issue).update(
            next_run_at=timezone.now() - timedelta(minutes=1)
        )

        assert scan_due_tickers() == 1
        assert delay.call_count == 1


# ---------------------------------------------------------------------------
# wait_ticker — the audit trail
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_each_wait_records_an_activity_entry_naming_the_calling_run(waiting_issue, runner):
    run = _finished_run(waiting_issue, runner)

    scheduling.wait_ticker(waiting_issue, run=run)
    scheduling.wait_ticker(waiting_issue, run=run)

    entries = list(
        IssueActivity.objects.filter(issue=waiting_issue, field=scheduling.WAIT_ACTIVITY_FIELD).order_by("created_at")
    )
    assert len(entries) == 2, "repeated waits must stay visible, not collapse"
    assert [e.new_value for e in entries] == ["1", "2"]
    # The allowance, so the UI can render "2 of 10" without project policy.
    assert entries[-1].old_value == "10"
    assert str(run.pk) in entries[-1].comment


# ---------------------------------------------------------------------------
# what the platform no longer does
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_closing_a_blocker_fires_no_run_on_its_dependents(seeded, make_issue, states, runner, create_user):
    """The dependent picks the change up on its next cadence tick instead.

    This is the accepted latency cost of keeping ``blocked_by`` semantics out
    of the scheduler (PDASHOSS01-204).
    """
    blocker = make_issue("A", state="in_progress")
    dependent = make_issue("B", state="in_progress")
    _block(dependent, blocker)
    _finished_run(dependent, runner)
    runs_before = AgentRun.objects.filter(work_item=dependent).count()

    with impersonate(create_user):
        blocker.state = states["done"]
        blocker.save(update_fields=["state"])

    assert AgentRun.objects.filter(work_item=dependent).count() == runs_before
    assert not IssueActivity.objects.filter(issue=dependent, field="agent_wake").exists()


@pytest.mark.unit
def test_an_agent_that_decides_to_proceed_gets_no_interference(waiting_issue, make_issue, states, runner):
    """An open blocker plus a ``Waiting on:`` workpad line changes nothing.

    The prose is a note to the agent's future self. Nothing reads it, so the
    cadence tick fires exactly as it would with an empty workpad.
    """
    blocker = make_issue("A", state="in_progress")
    _block(waiting_issue, blocker)
    _finished_run(waiting_issue, runner)
    Issue.all_objects.filter(pk=waiting_issue.pk).update(
        workpad=f"### Notes\n\n- Waiting on: WEB-{blocker.sequence_id}\n"
    )
    IssueAgentTicker.objects.filter(issue=waiting_issue).update(next_run_at=timezone.now() - timedelta(minutes=1))
    ticker_id = str(IssueAgentTicker.objects.get(issue=waiting_issue).pk)

    with mock.patch(
        "pi_dash.orchestration.scheduling.dispatch_continuation_run",
        return_value=mock.Mock(pk="run-1"),
    ) as dispatch:
        assert fire_tick(ticker_id) is True

    assert dispatch.call_count == 1, "the marker must not pause the clock"
    # The tick was spent normally: nothing was refunded, because nothing asked.
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert ticker.used == 4
    assert ticker.waited == 0


@pytest.mark.unit
def test_no_scheduling_code_reads_the_waiting_on_marker():
    """Grep the shipped scheduler for the marker: prompt text only.

    The point of this issue is that the workpad stopped being an API. A
    parser reintroduced anywhere under ``orchestration`` / ``bgtasks`` would
    quietly restore it, and no behavioural test would necessarily catch it.
    """
    import pathlib

    import pi_dash

    root = pathlib.Path(pi_dash.__file__).parent
    # The marker as an agent writes it, and the parser that used to read it.
    # Migrations are excluded: they document the removal, which is the point.
    needles = ("waiting on:", "parse_waiting_on")
    offenders = []
    for area in ("orchestration", "bgtasks", "api", "db", "runner"):
        for path in (root / area).rglob("*.py"):
            if "migrations" in path.parts or "__pycache__" in path.parts:
                continue
            body = path.read_text().lower()
            if any(needle in body for needle in needles):
                offenders.append(str(path.relative_to(root)))
    assert offenders == []


# ---------------------------------------------------------------------------
# end to end: waiting on the last tick of the pool
# ---------------------------------------------------------------------------


def _spend_the_pool(issue):
    """Leave the ticker exactly as ``fire_tick``'s claim leaves it when the
    tick it just granted was the last one in the pool."""
    ticker = IssueAgentTicker.objects.get(issue=issue)
    ticker.used = 10
    ticker.enabled = False
    ticker.disarm_reason = TickerDisarmReason.CAP_HIT
    ticker.next_run_at = None
    ticker.save(update_fields=["used", "enabled", "disarm_reason", "next_run_at"])
    return ticker


@pytest.mark.unit
def test_without_a_wait_the_spent_pool_parks_the_issue(waiting_issue, runner, states):
    """The baseline the wait has to overturn — CAP_HIT means Paused."""
    _spend_the_pool(waiting_issue)
    run = _finished_run(waiting_issue, runner)

    assert scheduling.maybe_apply_deferred_pause(run) is True

    waiting_issue.refresh_from_db()
    assert waiting_issue.state.name == "Paused"


@pytest.mark.unit
def test_waiting_on_the_last_tick_leaves_the_issue_in_progress(waiting_issue, runner, states):
    """The whole point, end to end: a run that ends by waiting costs nothing.

    The claim already disarmed at CAP_HIT, so the wait has to both refund the
    tick and clear that reason — otherwise the terminate hook parks the issue
    and the refunded tick is never spent.
    """
    _spend_the_pool(waiting_issue)
    run = _finished_run(waiting_issue, runner)

    assert scheduling.wait_ticker(waiting_issue, run=run)["applied"] is True
    assert scheduling.maybe_apply_deferred_pause(run) is False

    waiting_issue.refresh_from_db()
    assert waiting_issue.state.name == "In Progress"
    ticker = IssueAgentTicker.objects.get(issue=waiting_issue)
    assert ticker.enabled is True
    assert ticker.next_run_at is not None, "the next cadence tick must re-ask"
