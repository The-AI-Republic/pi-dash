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
        status=(RunnerStatus.ONLINE if online else RunnerStatus.OFFLINE),
        last_heartbeat_at=(timezone.now() - timezone.timedelta(seconds=heartbeat_ago_s) if online else None),
    )


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    """Replace the WS send so tests don't need redis/channels.

    Patches at the source (``pubsub.send_to_runner``) because matcher.py
    imports it lazily inside ``drain_pod`` to avoid an import cycle.
    """
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
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
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


# ---------------- select_runner_in_pod ----------------


@pytest.mark.unit
def test_select_runner_in_pod_none_when_empty(db, pod):
    from django.db import transaction

    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


@pytest.mark.unit
def test_select_runner_in_pod_picks_freshest_heartbeat(db, create_user, workspace, pod):
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
def test_select_runner_in_pod_excludes_other_pod(db, create_user, workspace, project):
    """Runners in a different pod within the same project are not selected."""
    from django.db import transaction

    pod_a = Pod.objects.create(project=project, name=f"{project.identifier}_a", created_by=create_user)
    pod_b = Pod.objects.create(project=project, name=f"{project.identifier}_b", created_by=create_user)
    _make_runner(create_user, workspace, pod_b, "only-in-b")
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod_a) is None


@pytest.mark.unit
def test_select_runner_in_pod_skips_stale_heartbeat(db, create_user, workspace, pod):
    from django.db import transaction

    _make_runner(create_user, workspace, pod, "stale", heartbeat_ago_s=120)
    with transaction.atomic():
        assert matcher.select_runner_in_pod(pod) is None


# ---------- worktree capacity hint retired (PDASHOSS01-137) ----------
# ``free_worktrees`` is no longer written or read; the matcher ranks eligible
# idle runners by freshest heartbeat only. A leftover column value on a
# historical row must not influence selection.


@pytest.mark.unit
def test_select_runner_in_pod_ranks_by_heartbeat_ignoring_free_worktrees(db, create_user, workspace, pod):
    """The freshest-heartbeat runner wins even when an older runner carries a
    non-zero ``free_worktrees`` value — the retired hint no longer ranks."""
    from django.db import transaction

    fresh = _make_runner(create_user, workspace, pod, "fresh", heartbeat_ago_s=1)
    fresh.free_worktrees = 0
    fresh.save(update_fields=["free_worktrees"])

    older_with_stale_hint = _make_runner(create_user, workspace, pod, "older", heartbeat_ago_s=20)
    older_with_stale_hint.free_worktrees = 2
    older_with_stale_hint.save(update_fields=["free_worktrees"])

    with transaction.atomic():
        picked = matcher.select_runner_in_pod(pod)
    assert picked.pk == fresh.pk


@pytest.mark.unit
def test_select_runner_in_pod_selects_runner_with_zero_free_worktrees(db, create_user, workspace, pod):
    """A stale ``free_worktrees == 0`` is not a gate — the sole eligible idle
    runner is still selected."""
    from django.db import transaction

    full = _make_runner(create_user, workspace, pod, "full")
    full.free_worktrees = 0
    full.save(update_fields=["free_worktrees"])
    with transaction.atomic():
        picked = matcher.select_runner_in_pod(pod)
    assert picked.pk == full.pk


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
def test_drain_pod_assigns_queued_to_idle_runner(db, create_user, workspace, pod, _stub_send_to_runner):
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
def test_private_runner_does_not_take_other_users_run(db, create_user, workspace, pod, _stub_send_to_runner):
    from uuid import uuid4

    from pi_dash.db.models import User, WorkspaceMember

    _make_runner(create_user, workspace, pod, "owner-runner")
    other = User.objects.create(
        email=f"other-{uuid4().hex[:8]}@example.com",
        username=f"other_{uuid4().hex[:8]}",
    )
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=other,
        created_by=other,
        pod=pod,
        status=AgentRunStatus.QUEUED,
        prompt="not mine",
    )

    n = matcher.drain_pod(pod)

    assert n == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None
    assert not _stub_send_to_runner.called


@pytest.mark.unit
def test_drain_pod_stops_when_all_runners_busy(db, create_user, workspace, pod, _stub_send_to_runner):
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
def test_drain_pod_noop_when_no_runners(db, create_user, workspace, pod, _stub_send_to_runner):
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


def _make_run(user, workspace, pod, *, prompt="x", pinned_runner=None, status=AgentRunStatus.QUEUED):
    return AgentRun.objects.create(
        owner=user,
        created_by=user,
        workspace=workspace,
        pod=pod,
        prompt=prompt,
        status=status,
        pinned_runner=pinned_runner,
    )


@pytest.mark.unit
def test_next_for_runner_prefers_personal_queue(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    # Older unpinned run + newer run pinned to me → pinned wins.
    older = _make_run(create_user, workspace, pod, prompt="older unpinned")
    pinned = _make_run(create_user, workspace, pod, prompt="mine", pinned_runner=rA)
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    assert nxt is not None
    assert nxt.id == pinned.id
    assert older.id != pinned.id  # sanity: older exists, just not chosen


@pytest.mark.unit
def test_next_for_runner_falls_back_to_pod_queue(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    unpinned = _make_run(create_user, workspace, pod, prompt="any")
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    assert nxt is not None
    assert nxt.id == unpinned.id


@pytest.mark.unit
def test_next_for_runner_excludes_pinned_to_others(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_run(create_user, workspace, pod, prompt="for B", pinned_runner=rB)
    from django.db import transaction

    with transaction.atomic():
        nxt = matcher.next_for_runner(rA)
    # rA must not pick up a run pinned to rB.
    assert nxt is None


@pytest.mark.unit
def test_drain_pod_skips_pinned_to_busy_runner(db, create_user, workspace, pod):
    """Head-of-line is not blocked when the head run is pinned to a busy runner."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    # Make rA busy.
    AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        prompt="agentA's current",
        runner=rA,
        status=AgentRunStatus.RUNNING,
    )
    # Older pinned-to-busy-A run + newer unpinned run.
    pinned_for_a = _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA)
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
def test_drain_for_runner_picks_personal_first(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    older_unpinned = _make_run(create_user, workspace, pod, prompt="any")
    pinned = _make_run(create_user, workspace, pod, prompt="mine", pinned_runner=rA)

    assigned = matcher.drain_for_runner(rA)
    assert assigned is True
    pinned.refresh_from_db()
    older_unpinned.refresh_from_db()
    assert pinned.runner_id == rA.id
    assert pinned.status == AgentRunStatus.ASSIGNED
    # Older unpinned run should still be QUEUED — drain_for_runner takes one.
    assert older_unpinned.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_drain_for_runner_returns_false_when_busy(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        prompt="busy",
        runner=rA,
        status=AgentRunStatus.RUNNING,
    )
    _make_run(create_user, workspace, pod, prompt="waiting", pinned_runner=rA)
    assert matcher.drain_for_runner(rA) is False


# ---------------------------------------------------------------------------
# Prompt freshness at dispatch.
#
# Every run's prompt is rendered at run-creation time and is *never* rebuilt
# at dispatch — neither for first-turn runs nor for follow-up runs. The agent
# reconstructs context (new comments, current workpad, repo state) at runtime
# via the `pidash` CLI; the dispatcher's job is to deliver, not re-render.
# See ``.ai_design/ticking_optimization/design.md``.
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_drain_for_runner_does_not_rebuild_prompt_for_followup(db, create_user, workspace, pod):
    """Follow-up runs ship the prompt as recorded at creation time.

    A late-arriving comment between run-creation and dispatch is *not* swept
    into the prompt — the agent reads the comment thread at runtime instead.
    """
    from crum import impersonate

    from pi_dash.db.models.issue import Issue, IssueComment
    from pi_dash.db.models.project import Project
    from pi_dash.db.models.state import State

    rA = _make_runner(create_user, workspace, pod, "rA")
    with impersonate(create_user):
        project = Project.objects.create(name="P", identifier="P", workspace=workspace, created_by=create_user)
        todo = State.objects.create(name="Todo", project=project, group="unstarted")
        issue = Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=todo,
            created_by=create_user,
        )
    parent = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
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
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue,
        parent_run=parent,
        pinned_runner=rA,
        status=AgentRunStatus.QUEUED,
        prompt="prompt-rendered-at-creation",
    )
    with impersonate(create_user):
        IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=create_user,
            comment_html="<p>arrived after queue</p>",
        )

    assert matcher.drain_for_runner(rA) is True
    queued.refresh_from_db()
    assert queued.prompt == "prompt-rendered-at-creation"


@pytest.mark.unit
def test_drain_does_not_rebuild_first_turn_prompt(db, create_user, workspace, pod):
    """A fresh run (parent_run is None) keeps its as-stored prompt."""
    rA = _make_runner(create_user, workspace, pod, "rA")
    run = _make_run(create_user, workspace, pod, prompt="original first-turn body")
    assert matcher.drain_for_runner(rA) is True
    run.refresh_from_db()
    assert run.prompt == "original first-turn body"


# ---------------------------------------------------------------------------
# Pin wait budget: a pin to a healthy-but-busy runner is bounded.
# See PDASHOSS01-272, ``matcher.releasable_overbudget_pin_ids`` and
# ``matcher.next_assignable_for_runner``.
# ---------------------------------------------------------------------------


def _backdate(run, seconds):
    """Age a run's ``created_at``; the column is ``auto_now_add``."""
    stamp = timezone.now() - timezone.timedelta(seconds=seconds)
    AgentRun.objects.filter(pk=run.pk).update(created_at=stamp)
    run.refresh_from_db()
    return run


def _make_busy(user, workspace, pod, runner):
    return AgentRun.objects.create(
        owner=user,
        created_by=user,
        workspace=workspace,
        pod=pod,
        prompt=f"{runner.name}'s current",
        runner=runner,
        status=AgentRunStatus.RUNNING,
    )


@pytest.mark.unit
def test_drain_pod_releases_overbudget_pin_to_idle_runner(db, create_user, workspace, pod):
    """The reported incident: pinned to a healthy busy runner, idle runners waiting."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        3600,
    )

    n = matcher.drain_pod(pod)
    pinned.refresh_from_db()
    assert n == 1
    assert pinned.status == AgentRunStatus.ASSIGNED
    assert pinned.runner_id == rB.id
    assert pinned.pinned_runner_id is None


@pytest.mark.unit
def test_drain_pod_keeps_pin_within_budget(db, create_user, workspace, pod):
    """Inside the budget the pin is strict — the run waits for its own runner."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    # Aged, but well inside the 600s default.
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        60,
    )

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.status == AgentRunStatus.QUEUED
    assert pinned.pinned_runner_id == rA.id
    assert pinned.runner_id is None


@pytest.mark.unit
def test_drain_pod_keeps_overbudget_pin_when_no_runner_idle(db, create_user, workspace, pod):
    """No idle runner to hand it to → the pin survives, however long the wait."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    _make_busy(create_user, workspace, pod, rB)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.status == AgentRunStatus.QUEUED
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_drain_pod_honours_overbudget_pin_to_idle_runner(db, create_user, workspace, pod):
    """The pinned runner is itself idle: it keeps its own run, budget or not.

    ``rB`` has the fresher heartbeat so ``drain_pod`` offers work to it first;
    it must not steal the run that ``rA`` is about to take.
    """
    rA = _make_runner(create_user, workspace, pod, "agentA", heartbeat_ago_s=10)
    rB = _make_runner(create_user, workspace, pod, "agentB", heartbeat_ago_s=1)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_pod(pod) == 1
    pinned.refresh_from_db()
    assert pinned.runner_id == rA.id
    assert pinned.status == AgentRunStatus.ASSIGNED
    assert rB.id != rA.id


@pytest.mark.unit
def test_pod_budget_override_of_zero_disables_auto_release(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        86400,
    )
    pod.pin_wait_budget_secs = 0
    pod.save(update_fields=["pin_wait_budget_secs"])

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_pod_budget_override_shortens_the_wait(db, create_user, workspace, pod):
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        60,
    )
    # 60s of waiting is inside the 600s instance default but past this pod's
    # own 30s budget.
    pod.pin_wait_budget_secs = 30
    pod.save(update_fields=["pin_wait_budget_secs"])

    assert matcher.drain_pod(pod) == 1
    pinned.refresh_from_db()
    assert pinned.runner_id == rB.id
    assert pinned.pinned_runner_id is None


@pytest.mark.unit
def test_effective_pin_wait_budget_falls_back_to_instance_default(db, settings, pod):
    settings.RUNNER_PIN_WAIT_BUDGET_SECS = 900
    assert pod.pin_wait_budget_secs is None
    assert pod.effective_pin_wait_budget_secs() == 900
    pod.pin_wait_budget_secs = 120
    assert pod.effective_pin_wait_budget_secs() == 120


@pytest.mark.unit
def test_auto_release_clears_parent_thread_id(db, create_user, workspace, pod):
    """Mirror the release-pin endpoint: the new runner gets no resume hint."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    parent = _make_busy(create_user, workspace, pod, rA)
    parent.thread_id = "thread-abc"
    parent.save(update_fields=["thread_id"])
    pinned = _make_run(create_user, workspace, pod, prompt="follow-up", pinned_runner=rA)
    pinned.parent_run = parent
    pinned.save(update_fields=["parent_run"])
    _backdate(pinned, 3600)

    assert matcher.drain_pod(pod) == 1
    pinned.refresh_from_db()
    parent.refresh_from_db()
    assert pinned.runner_id == rB.id
    assert pinned.pinned_runner_id is None
    assert parent.thread_id == ""


@pytest.mark.unit
def test_managed_run_pin_is_never_auto_released(db, create_user, workspace, pod):
    """An unpinned managed run is unservable, so its pin must hold."""
    from pi_dash.core.agent_execution import AgentExecutorKind

    rA = _make_runner(create_user, workspace, pod, "agentA")
    _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _make_run(create_user, workspace, pod, prompt="managed", pinned_runner=rA)
    AgentRun.objects.filter(pk=pinned.pk).update(executor_kind=AgentExecutorKind.MANAGED_RUNNER)
    _backdate(pinned, 3600)

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_desktop_bundled_runner_does_not_claim_released_pin(db, create_user, workspace, pod):
    """A user's laptop must not become the pod's overflow build server."""
    from pi_dash.runner.models import RunnerProvisioning

    rA = _make_runner(create_user, workspace, pod, "agentA")
    desktop = _make_runner(create_user, workspace, pod, "desktop")
    desktop.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    desktop.save(update_fields=["provisioning"])
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_released_pin_still_respects_runner_eligibility(db, create_user, workspace, pod):
    """Releasing a pin does not widen who may run the work.

    ``other``'s private runner cannot see a run created by and billed to
    ``create_user``, so the pin stays put even though the budget is spent.
    """
    from uuid import uuid4

    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(
        email=f"other-{uuid4().hex[:8]}@example.com",
        username=f"other_{uuid4().hex[:8]}",
    )
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    rA = _make_runner(create_user, workspace, pod, "agentA")
    _make_runner(other, workspace, pod, "outsiders-runner")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_pod(pod) == 0
    pinned.refresh_from_db()
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_drain_for_runner_claims_overbudget_pin(db, create_user, workspace, pod):
    """The single-runner immediate-dispatch path releases the pin too."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="for A later", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_for_runner(rB) is True
    pinned.refresh_from_db()
    assert pinned.runner_id == rB.id
    assert pinned.pinned_runner_id is None


@pytest.mark.unit
def test_overbudget_release_is_fifo_across_two_idle_runners(db, create_user, workspace, pod):
    """Two over-budget pins, two idle runners: the older one goes out first."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB", heartbeat_ago_s=1)
    rC = _make_runner(create_user, workspace, pod, "agentC", heartbeat_ago_s=5)
    _make_busy(create_user, workspace, pod, rA)
    older = _backdate(_make_run(create_user, workspace, pod, prompt="older", pinned_runner=rA), 7200)
    newer = _backdate(_make_run(create_user, workspace, pod, prompt="newer", pinned_runner=rA), 3600)

    assert matcher.drain_pod(pod) == 2
    older.refresh_from_db()
    newer.refresh_from_db()
    # rB is offered work first (freshest heartbeat) and takes the older run.
    assert older.runner_id == rB.id
    assert newer.runner_id == rC.id
    assert older.pinned_runner_id is None
    assert newer.pinned_runner_id is None


# ---------------------------------------------------------------------------
# An over-budget pin competes with the pod queue on age, rather than
# trailing behind it. An earlier shape released the pin only *after*
# ``next_for_runner`` came up empty, which made a released pin the
# lowest-priority work in the pod: on a congested pod the ticker's steady
# supply of newer unpinned runs took every idle slot ahead of it, so the
# starvation had no bound. See PDASHOSS01-272.
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_overbudget_pin_beats_a_newer_unpinned_run(db, create_user, workspace, pod):
    """One idle slot, and the over-budget pin is the older work: it wins."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="waited two hours", pinned_runner=rA),
        7200,
    )
    fresh = _make_run(create_user, workspace, pod, prompt="just queued")

    assert matcher.drain_pod(pod) == 1
    pinned.refresh_from_db()
    fresh.refresh_from_db()
    assert pinned.runner_id == rB.id
    assert pinned.pinned_runner_id is None
    assert fresh.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_older_unpinned_run_still_goes_before_an_overbudget_pin(db, create_user, workspace, pod):
    """FIFO cuts both ways: releasing a pin does not promote it past older work."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    older_unpinned = _backdate(_make_run(create_user, workspace, pod, prompt="oldest"), 7200)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="over budget", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_pod(pod) == 1
    older_unpinned.refresh_from_db()
    pinned.refresh_from_db()
    assert older_unpinned.runner_id == rB.id
    # Not taken, so not released either — the pin is only ever broken by the
    # runner that is actually about to serve the run.
    assert pinned.status == AgentRunStatus.QUEUED
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_runners_own_pinned_work_still_outranks_an_older_overbudget_pin(db, create_user, workspace, pod):
    """Tier 0 is untouched: rB's own pin beats an older pin released to it."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    stolen = _backdate(
        _make_run(create_user, workspace, pod, prompt="older, pinned to A", pinned_runner=rA),
        7200,
    )
    mine = _backdate(
        _make_run(create_user, workspace, pod, prompt="newer, pinned to B", pinned_runner=rB),
        3600,
    )

    assert matcher.drain_pod(pod) == 1
    stolen.refresh_from_db()
    mine.refresh_from_db()
    assert mine.runner_id == rB.id
    assert mine.pinned_runner_id == rB.id
    assert stolen.status == AgentRunStatus.QUEUED
    assert stolen.pinned_runner_id == rA.id


@pytest.mark.unit
def test_drain_for_runner_prefers_the_older_unpinned_run(db, create_user, workspace, pod):
    """Same ordering on the immediate-dispatch path."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    older_unpinned = _backdate(_make_run(create_user, workspace, pod, prompt="oldest"), 7200)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="over budget", pinned_runner=rA),
        3600,
    )

    assert matcher.drain_for_runner(rB) is True
    older_unpinned.refresh_from_db()
    pinned.refresh_from_db()
    assert older_unpinned.runner_id == rB.id
    assert pinned.pinned_runner_id == rA.id


@pytest.mark.unit
def test_within_budget_pin_never_competes_with_the_pod_queue(db, create_user, workspace, pod):
    """A pin inside its budget is invisible to other runners, oldest or not."""
    rA = _make_runner(create_user, workspace, pod, "agentA")
    rB = _make_runner(create_user, workspace, pod, "agentB")
    _make_busy(create_user, workspace, pod, rA)
    pinned = _backdate(
        _make_run(create_user, workspace, pod, prompt="oldest but pinned", pinned_runner=rA),
        60,
    )
    fresh = _make_run(create_user, workspace, pod, prompt="just queued")

    assert matcher.drain_pod(pod) == 1
    pinned.refresh_from_db()
    fresh.refresh_from_db()
    assert fresh.runner_id == rB.id
    assert pinned.status == AgentRunStatus.QUEUED
    assert pinned.pinned_runner_id == rA.id
