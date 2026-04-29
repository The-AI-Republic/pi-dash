# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the pod-scoped matcher helpers and ``drain_pod``.

Covers ``select_runner_in_pod``, ``next_queued_run_for_pod``, and
``drain_pod`` from ``pi_dash.runner.services.matcher``.
See ``.ai_design/issue_runner/design.md`` §6.3.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_runner(user, workspace, pod, name, online=True, heartbeat_ago_s=1):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        credential_hash=f"h-{name}",
        credential_fingerprint=name[:16].ljust(16, "x")[:16],
        status=(
            RunnerStatus.ONLINE if online else RunnerStatus.OFFLINE
        ),
        last_heartbeat_at=(
            timezone.now() - timezone.timedelta(seconds=heartbeat_ago_s)
            if online
            else None
        ),
    )


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    """Replace the WS send so tests don't need redis/channels.

    Patches at the source (``pubsub.send_to_runner``) because matcher.py
    imports it lazily inside ``drain_pod`` to avoid an import cycle.
    """
    with patch(
        "pi_dash.runner.services.pubsub.send_to_runner"
    ) as mock:
        yield mock


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """Run ``transaction.on_commit`` callbacks inline.

    pytest-django wraps each test in a transaction that gets rolled back,
    which means ``on_commit`` callbacks never fire. Patching the hook to
    execute immediately lets us verify side effects (WS dispatch, drain
    refiring) without switching the whole test to
    ``django_db(transaction=True)``.
    """
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


# ---------------- select_runner_in_pod ----------------


@pytest.mark.unit
def test_select_runner_in_pod_none_when_empty(db, pod):
    from django.db import transaction

    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_select_runner_in_pod_picks_freshest_heartbeat(
    db, create_user, workspace, pod
):
    from django.db import transaction

    _make_runner(create_user, workspace, pod, "old", heartbeat_ago_s=20)
    fresh = _make_runner(create_user, workspace, pod, "fresh", heartbeat_ago_s=1)
    with transaction.atomic():
        picked = matcher.select_runner_in_pod(pod)
    assert picked.pk == fresh.pk


@pytest.mark.unit
def test_select_runner_in_pod_excludes_busy(db, create_user, workspace, pod):
    from django.db import transaction

    busy = _make_runner(create_user, workspace, pod, "busy")
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=busy,
        status=AgentRunStatus.RUNNING,
        prompt="x",
    )
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_select_runner_in_pod_excludes_other_pod(
    db, create_user, workspace
):
    """Runners in a different pod in the same workspace are not selected."""
    from django.db import transaction

    pod_a = Pod.objects.create(
        workspace=workspace, name="a", created_by=create_user
    )
    pod_b = Pod.objects.create(
        workspace=workspace, name="b", created_by=create_user
    )
    _make_runner(create_user, workspace, pod_b, "only-in-b")
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod_a) is None


@pytest.mark.unit
def test_select_runner_in_pod_skips_stale_heartbeat(
    db, create_user, workspace, pod
):
    from django.db import transaction

    _make_runner(create_user, workspace, pod, "stale", heartbeat_ago_s=120)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


# ---------------- next_queued_run_for_pod ----------------


@pytest.mark.unit
def test_next_queued_run_fifo(db, create_user, workspace, pod):
    from django.db import transaction

    first = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="first",
    )
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="second",
    )
    with transaction.atomic():
        assert matcher.next_queued_run_for_pod(pod).pk == first.pk


# ---------------- drain_pod ----------------


@pytest.mark.unit
def test_drain_pod_assigns_queued_to_idle_runner(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    runner = _make_runner(create_user, workspace, pod, "r1")
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="go",
    )
    n = matcher.drain_pod(pod)
    assert n == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == runner.pk
    # Billing capture: AgentRun.owner now reflects the runner's owner.
    assert run.owner_id == runner.owner_id
    # Dispatch WS call was fired.
    assert _stub_send_to_runner.called


@pytest.mark.unit
def test_drain_pod_stops_when_all_runners_busy(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    _make_runner(create_user, workspace, pod, "only-runner")
    run_a = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="a",
    )
    run_b = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="b",
    )
    n = matcher.drain_pod(pod)
    assert n == 1  # Only the one runner got filled.
    run_a.refresh_from_db()
    run_b.refresh_from_db()
    statuses = sorted([run_a.status, run_b.status])
    assert statuses == [AgentRunStatus.ASSIGNED, AgentRunStatus.QUEUED]


@pytest.mark.unit
def test_drain_pod_noop_when_no_runners(
    db, create_user, workspace, pod, _stub_send_to_runner
):
    AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="orphan",
    )
    n = matcher.drain_pod(pod)
    assert n == 0
    assert not _stub_send_to_runner.called


@pytest.mark.unit
def test_drain_pod_by_id_returns_zero_when_missing(db):
    import uuid

    assert matcher.drain_pod_by_id(uuid.uuid4()) == 0


# ---------------------------------------------------------------------------
# Pinning model: per-runner personal queue + pod general queue.
# See §5 of .ai_design/issue_run_improve/design.md.
# ---------------------------------------------------------------------------


def _make_run(user, workspace, pod, *, prompt="x", pinned_runner=None,
              status=AgentRunStatus.QUEUED):
    return AgentRun.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        prompt=prompt,
        status=status,
        pinned_runner=pinned_runner,
    )


@pytest.mark.unit
def test_next_for_runner_prefers_personal_queue(
    db, create_user, workspace, pod
):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    # Older unpinned run + newer run pinned to me → pinned wins.
    older = _make_run(create_user, workspace, pod, prompt="older unpinned")
    pinned = _make_run(
        create_user, workspace, pod, prompt="mine", pinned_runner=rA
    )
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    assert nxt is not None
    assert nxt.id == pinned.id
    assert older.id != pinned.id  # sanity: older exists, just not chosen


@pytest.mark.unit
def test_next_for_runner_falls_back_to_pod_queue(
    db, create_user, workspace, pod
):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    unpinned = _make_run(create_user, workspace, pod, prompt="any")
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    assert nxt is not None
    assert nxt.id == unpinned.id


@pytest.mark.unit
def test_next_for_runner_excludes_pinned_to_others(
    db, create_user, workspace, pod
):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_run(create_user, workspace, pod, prompt="for B", pinned_runner=rB)
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    # rA must not pick up a run pinned to rB.
    assert nxt is None


@pytest.mark.unit
def test_drain_pod_skips_pinned_to_busy_runner(
    db, create_user, workspace, pod
):
    """Head-of-line is not blocked when the head run is pinned to a busy runner."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    # Make rA busy.
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        prompt="agentA's current",
        runner=rA,
        status=AgentRunStatus.RUNNING,
    )
    # Older pinned-to-busy-A run + newer unpinned run.
    pinned_for_a = _make_run(
        create_user, workspace, pod, prompt="for A later", pinned_runner=rA
    )
    unpinned = _make_run(create_user, workspace, pod, prompt="anyone")

    n = matcher.drain_pod(pod)
    pinned_for_a.refresh_from_db()
    unpinned.refresh_from_db()
    # rB should pick up the unpinned run; pinned-for-A should still be QUEUED.
    assert n == 1
    assert unpinned.runner_id == rB.id
    assert unpinned.status == AgentRunStatus.ASSIGNED
    assert pinned_for_a.status == AgentRunStatus.QUEUED
    assert pinned_for_a.runner_id is None


@pytest.mark.unit
def test_drain_for_runner_picks_personal_first(
    db, create_user, workspace, pod
):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    older_unpinned = _make_run(create_user, workspace, pod, prompt="any")
    pinned = _make_run(
        create_user, workspace, pod, prompt="mine", pinned_runner=rA
    )

    assigned = matcher.drain_for_runner(rA)
    assert assigned is True
    pinned.refresh_from_db()
    older_unpinned.refresh_from_db()
    assert pinned.runner_id == rA.id
    assert pinned.status == AgentRunStatus.ASSIGNED
    # Older unpinned run should still be QUEUED — drain_for_runner takes one.
    assert older_unpinned.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_drain_for_runner_returns_false_when_busy(
    db, create_user, workspace, pod
):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        prompt="busy",
        runner=rA,
        status=AgentRunStatus.RUNNING,
    )
    _make_run(create_user, workspace, pod, prompt="waiting", pinned_runner=rA)
    assert matcher.drain_for_runner(rA) is False


# ---------------------------------------------------------------------------
# Continuation prompt freshness at dispatch.
#
# Coalescing logic in ``handle_issue_comment`` returns ``coalesced`` for a
# second comment that arrives while R_next is still QUEUED. The contract is
# that the prompt builder runs again at dispatch time so the runner sees both
# bodies. These tests pin that contract.
# ---------------------------------------------------------------------------


def _setup_paused_chain(create_user, workspace, pod, project_factory=None):
    """Create an issue + paused parent run + queued continuation run.

    Returns (issue, parent_run, queued_followup, runner). The parent has
    a started_at in the past so build_continuation can sweep comments.

    The issue is created in an ``unstarted`` state so the orchestration
    state-transition signal does not auto-create a competing run that
    would consume the runner.
    """
    from crum import impersonate

    from pi_dash.db.models.issue import Issue
    from pi_dash.db.models.project import Project
    from pi_dash.db.models.state import State

    rA = _make_runner(create_user, workspace, pod, "rA")
    with impersonate(create_user):
        project = Project.objects.create(
            name="P", identifier="P", workspace=workspace, created_by=create_user
        )
        todo = State.objects.create(
            name="Todo", project=project, group="unstarted"
        )
        issue = Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=todo,
            created_by=create_user,
        )
    parent = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue,
        runner=rA,
        thread_id="sess_xyz",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )
    queued = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue,
        parent_run=parent,
        pinned_runner=rA,
        status=AgentRunStatus.QUEUED,
        prompt="(stale prompt — only first comment)",
    )
    return issue, parent, queued, rA


@pytest.mark.unit
def test_drain_for_runner_rebuilds_continuation_prompt(
    db, create_user, workspace, pod
):
    """Coalesced comment must reach the runner via prompt rebuild on dispatch."""
    from crum import impersonate

    from pi_dash.db.models.issue import IssueComment

    issue, parent, queued, rA = _setup_paused_chain(create_user, workspace, pod)
    # Two comments after parent.started_at — the second arrived after the
    # follow-up was queued but before dispatch (the coalescing case).
    with impersonate(create_user):
        IssueComment.objects.create(
            issue=issue, project=issue.project, workspace=issue.workspace,
            actor=create_user, comment_html="<p>first</p>",
        )
        IssueComment.objects.create(
            issue=issue, project=issue.project, workspace=issue.workspace,
            actor=create_user, comment_html="<p>second</p>",
        )

    assigned = matcher.drain_for_runner(rA)
    assert assigned is True
    queued.refresh_from_db()
    # Both bodies must be present in the dispatched prompt — the stale
    # at-creation render is replaced.
    assert "first" in queued.prompt
    assert "second" in queued.prompt


@pytest.mark.unit
def test_drain_pod_rebuilds_continuation_prompt(
    db, create_user, workspace, pod
):
    """Same contract for the pod-wide drain path."""
    from crum import impersonate

    from pi_dash.db.models.issue import IssueComment

    issue, parent, queued, rA = _setup_paused_chain(create_user, workspace, pod)
    with impersonate(create_user):
        IssueComment.objects.create(
            issue=issue, project=issue.project, workspace=issue.workspace,
            actor=create_user, comment_html="<p>late comment</p>",
        )

    n = matcher.drain_pod(pod)
    assert n == 1
    queued.refresh_from_db()
    assert "late comment" in queued.prompt


@pytest.mark.unit
def test_drain_does_not_rebuild_first_turn_prompt(
    db, create_user, workspace, pod
):
    """A fresh run (parent_run is None) keeps its as-stored prompt."""
    rA = _make_runner(create_user, workspace, pod, "rA")
    run = _make_run(create_user, workspace, pod, prompt="original first-turn body")
    assert matcher.drain_for_runner(rA) is True
    run.refresh_from_db()
    assert run.prompt == "original first-turn body"
