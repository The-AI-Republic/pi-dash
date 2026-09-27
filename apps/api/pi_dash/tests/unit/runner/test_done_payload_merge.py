# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""A ``pidash run yield`` must survive the run's own terminal payload.

The agent yields first (``done_payload = {status, note, yielded_at}``); the
bridge then reports completion with ``{"conclusion": …}`` (never a
``status``). ``finalize_agent_run`` / ``apply_run_paused`` merge rather than
replace, so ``reconcile(RUN_ENDED)`` still sees the yielded outcome.
"""

from __future__ import annotations

from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker, TickerDisarmReason
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus
from pi_dash.runner.services import run_lifecycle
from pi_dash.runner.services.agent_run_finalization import finalize_agent_run, merge_done_payload


@pytest.mark.unit
def test_merge_keeps_the_yield_under_a_bridge_payload():
    existing = {"status": "waiting_on_human", "note": "asked", "yielded_at": "2026-09-13T00:00:00Z"}
    incoming = {"conclusion": "success", "result": "final message"}
    merged = merge_done_payload(existing, incoming)
    assert merged["status"] == "waiting_on_human"
    assert merged["note"] == "asked"
    assert merged["yielded_at"] == "2026-09-13T00:00:00Z"
    assert merged["conclusion"] == "success"
    assert merged["result"] == "final message"


@pytest.mark.unit
def test_merge_lets_an_explicit_status_win():
    # A Cloud Agent structured result carries its own status.
    existing = {"status": "progressed", "yielded_at": "x"}
    incoming = {"status": "blocked", "summary": "…"}
    assert merge_done_payload(existing, incoming)["status"] == "blocked"


@pytest.mark.unit
def test_merge_without_a_yield_is_a_plain_replace():
    assert merge_done_payload(None, {"conclusion": "success"}) == {"conclusion": "success"}
    assert merge_done_payload({"status": "completed"}, {"conclusion": "x"}) == {"conclusion": "x"}
    assert merge_done_payload({"yielded_at": "x", "status": "done"}, None) == {"status": "done", "yielded_at": "x"}


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Merge", identifier="MRG", workspace=workspace, created_by=create_user, agent_default_max_ticks=10
        )


@pytest.fixture
def issue(workspace, project, create_user):
    with impersonate(create_user):
        todo = State.objects.create(name="Todo", project=project, group="unstarted")
        review = State.objects.create(name="In Review", project=project, group="review")
        i = Issue.objects.create(name="Task", workspace=workspace, project=project, state=todo, created_by=create_user)
    Issue.all_objects.filter(pk=i.pk).update(state=review)
    i.refresh_from_db()
    return i


@pytest.fixture
def runner(workspace, project, create_user):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=Pod.default_for_project(project),
        name="r",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _running(issue, runner, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        owner=runner.owner,
        pod=runner.pod,
        runner=runner,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="review",
        prompt="x",
        started_at=timezone.now(),
    )


@pytest.mark.unit
def test_yield_then_complete_applies_the_yield_to_the_clock(issue, runner, create_user):
    """End to end: a review run yields ``progressed``, the bridge completes it,
    and the clock keeps ticking (the per-kind default for review would have
    stopped it)."""
    IssueAgentTicker.objects.create(issue=issue, enabled=True, next_run_at=timezone.now())
    run = _running(issue, runner, create_user)
    run.done_payload = {"status": "progressed", "yielded_at": timezone.now().isoformat()}
    run.save(update_fields=["done_payload"])

    with mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()), mock.patch(
        "pi_dash.runner.services.agent_run_finalization.apply_agent_run_terminal_effects", create=True
    ):
        run_lifecycle.finalize_run_terminal(
            runner, run.id, AgentRunStatus.COMPLETED, done_payload={"conclusion": "success", "result": "ok"}
        )
    run.refresh_from_db()
    assert run.done_payload["status"] == "progressed"
    assert run.done_payload["conclusion"] == "success"
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.enabled is True


@pytest.mark.unit
def test_yield_then_complete_stops_the_clock_when_asked(issue, runner, create_user):
    IssueAgentTicker.objects.create(issue=issue, enabled=True, next_run_at=timezone.now())
    run = _running(issue, runner, create_user)
    run.done_payload = {"status": "waiting_on_human", "yielded_at": timezone.now().isoformat()}
    run.save(update_fields=["done_payload"])
    with mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        assert finalize_agent_run(run.id, AgentRunStatus.COMPLETED, updates={"done_payload": {"conclusion": "success"}})
    run.refresh_from_db()
    assert run.done_payload["status"] == "waiting_on_human"
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.enabled is False
    assert ticker.disarm_reason == TickerDisarmReason.TERMINAL_SIGNAL


@pytest.mark.unit
def test_pause_keeps_the_yield(issue, runner, create_user):
    run = _running(issue, runner, create_user)
    run.done_payload = {"status": "done", "yielded_at": "2026-09-13T00:00:00Z"}
    run.save(update_fields=["done_payload"])
    with mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        run_lifecycle.apply_run_paused(runner, run.id, {"autonomy": {"question_for_human": "?"}, "summary": "s"})
    run.refresh_from_db()
    assert run.status == AgentRunStatus.PAUSED_AWAITING_INPUT
    assert run.done_payload["status"] == "done"
    assert run.done_payload["autonomy"]["question_for_human"] == "?"
