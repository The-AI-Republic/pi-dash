# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentChatSession,
    AgentChatSessionStatus,
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


@pytest.fixture
def online_runner(db, create_user, workspace):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="laptop",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def queued_run(db, create_user, workspace):
    return AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="do X",
        status=AgentRunStatus.QUEUED,
    )


@pytest.mark.unit
def test_matcher_picks_online_idle_runner(db, online_runner, queued_run):
    assert matcher.select_runner_for_run(queued_run) == online_runner


@pytest.mark.unit
def test_matcher_ignores_offline(db, online_runner, queued_run):
    online_runner.status = RunnerStatus.OFFLINE
    online_runner.save(update_fields=["status"])
    assert matcher.select_runner_for_run(queued_run) is None


@pytest.mark.unit
def test_matcher_ignores_busy_runner(db, online_runner, queued_run):
    AgentRun.objects.create(
        owner=queued_run.owner,
        workspace=queued_run.workspace,
        prompt="already in flight",
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
    )
    assert matcher.select_runner_for_run(queued_run) is None


@pytest.mark.unit
def test_matcher_includes_runner_with_active_chat_turn(db, online_runner, queued_run, create_user, workspace):
    # An active chat turn no longer excludes a runner from issue assignment:
    # chat and issue runs are concurrent (the runner serves chat in a dedicated
    # worktree, separate from the issue pool). See design
    # make_chat_issue_parallel_working §3.4.
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        active_turn_id="turn_123",
    )
    assert matcher.select_runner_for_run(queued_run) == online_runner


@pytest.mark.unit
def test_matcher_includes_runner_with_active_chat_message(db, online_runner, queued_run, create_user, workspace):
    # As above: an in-flight chat message does not block issue assignment.
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        active_message_id=queued_run.id,
    )
    assert matcher.select_runner_for_run(queued_run) == online_runner


# ---------------------------------------------------------------------------
# Regression: chat-session exclude across multiple rows
#
# The pre-fix matcher chained two ``.exclude(chat_sessions__...)`` calls.
# Django compiled each to its own EXISTS subquery, so the conditions could
# be satisfied by *different* chat_session rows on the same runner —
# over-excluding when a runner had a stale CLOSED session plus an idle
# OPEN session. The fix collapses both clauses into a single subquery so
# all conditions evaluate on the same row.
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_matcher_includes_runner_with_closed_session_and_clean_open_session(
    db, online_runner, queued_run, create_user, workspace
):
    """Runner is eligible when its only OPEN session has no active turn,
    even if a separate CLOSED session was never cleared of its
    ``active_message_id`` / ``active_turn_id``.

    Pre-fix bug: two chained ``.exclude()`` clauses on ``chat_sessions__*``
    matched the CLOSED row's active_message_id and the OPEN row's status
    independently, falsely excluding the runner. After fix, both
    conditions must match on the same row, so a CLOSED row can't combine
    with an OPEN row.
    """
    # Historical CLOSED session with stale active fields (the kind of row
    # left behind when chat-state cleanup didn't run).
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        status=AgentChatSessionStatus.CLOSED,
        active_message_id=queued_run.id,
        active_turn_id="stale_turn",
    )
    # Current OPEN session that is idle (no active turn).
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        status=AgentChatSessionStatus.OPEN,
    )
    assert matcher.select_runner_for_run(queued_run) == online_runner


@pytest.mark.unit
def test_drain_pod_includes_runner_with_closed_session_and_clean_open_session(
    db, online_runner, queued_run, create_user, workspace
):
    """Same regression check via the pod-scoped dispatcher path."""
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        status=AgentChatSessionStatus.CLOSED,
        active_message_id=queued_run.id,
    )
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        status=AgentChatSessionStatus.OPEN,
    )
    # Pod-scope the queued run so drain_pod can see it.
    queued_run.pod = online_runner.pod
    queued_run.save(update_fields=["pod"])
    assigned = matcher.drain_pod(online_runner.pod)
    assert assigned == 1
    queued_run.refresh_from_db()
    assert queued_run.runner_id == online_runner.id


@pytest.mark.unit
def test_matcher_includes_runner_with_only_closed_sessions_having_stale_fields(
    db, online_runner, queued_run, create_user, workspace
):
    """Sanity: CLOSED sessions alone — even with stale active fields —
    never block the matcher. (Same logical case as above without the
    second OPEN row, to pin down that the open row isn't required for
    inclusion.)"""
    AgentChatSession.objects.create(
        workspace=workspace,
        runner=online_runner,
        created_by=create_user,
        pod=online_runner.pod,
        status=AgentChatSessionStatus.CLOSED,
        active_message_id=queued_run.id,
        active_turn_id="stale",
    )
    assert matcher.select_runner_for_run(queued_run) == online_runner


@pytest.mark.unit
def test_cap_enforced(db, create_user, workspace):
    for i in range(Runner.MAX_PER_USER):
        Runner.objects.create(
            owner=create_user,
            workspace=workspace,
            name=f"r{i}",
        )
    assert matcher.can_register_another(create_user.id, workspace.id) is False
    assert matcher.count_active(create_user.id, workspace.id) == Runner.MAX_PER_USER


# ---------------------------------------------------------------------------
# Status classifier coverage for PAUSED_AWAITING_INPUT
#
# PAUSED is a non-terminal state where the runner is free to pick up other
# work while waiting for human reply (§4.3 of the design doc). The five
# classifiers must reflect that consistently.
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_paused_in_non_terminal_statuses():
    # Pod deletion must wait for paused runs to drain.
    assert AgentRunStatus.PAUSED_AWAITING_INPUT in matcher.NON_TERMINAL_STATUSES


@pytest.mark.unit
def test_paused_not_in_busy_statuses():
    # Runner is free to take other pod work while a paused run exists.
    assert AgentRunStatus.PAUSED_AWAITING_INPUT not in matcher.BUSY_STATUSES


@pytest.mark.unit
def test_paused_not_terminal_on_agent_run(db, create_user, workspace):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="paused",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
    )
    assert run.is_terminal is False


@pytest.mark.unit
def test_paused_not_active_on_agent_run(db, create_user, workspace):
    # The single-active-run guardrail in _active_run_for must permit a
    # follow-up run to be created when the prior one is paused.
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="paused",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
    )
    assert run.is_active is False


@pytest.mark.unit
def test_paused_not_in_metrics_active_statuses():
    from pi_dash.runner.views.metrics import ACTIVE_RUN_STATUSES

    # Operational metric tracks runs occupying runner capacity; paused doesn't.
    assert AgentRunStatus.PAUSED_AWAITING_INPUT not in ACTIVE_RUN_STATUSES


@pytest.mark.unit
def test_paused_runner_not_busy_for_matcher(
    db, online_runner, create_user, workspace
):
    # A paused run on the runner must not exclude it from matching.
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="parked",
        runner=online_runner,
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
    )
    fresh = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="fresh",
        status=AgentRunStatus.QUEUED,
    )
    assert matcher.select_runner_for_run(fresh) == online_runner
