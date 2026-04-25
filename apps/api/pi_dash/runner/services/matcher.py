# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner selection and pod dispatch.

Pod-scoped helpers (``select_runner_in_pod``, ``next_queued_run_for_pod``,
``drain_pod``) are the forward-looking API and are used by Phase 3 and beyond.
``select_runner_for_run`` is kept for back-compat with existing views /
orchestration code that Phase 1 has not yet migrated; Phase 3 will remove it.

See ``.ai_design/issue_runner/design.md`` §6.
"""

from __future__ import annotations

import logging
from datetime import timedelta
from typing import Optional

from django.db import models, transaction
from django.db.models import Q
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)

logger = logging.getLogger(__name__)

# A runner is considered stale (no longer eligible) if it has not heartbeated
# within this window. Defensive in case a runner crashed without calling Bye.
HEARTBEAT_GRACE = timedelta(seconds=90)

# Runs in these statuses occupy a runner slot or a queue position. Used to
# exclude busy runners from matching and to block pod deletion while work is
# outstanding.
#
# PAUSED_AWAITING_INPUT is non-terminal (the run will resume on a comment) so
# it gates pod deletion — but it is NOT in BUSY_STATUSES because the runner
# is free to take other pod work while waiting for human reply. See §4.3 of
# .ai_design/issue_run_improve/design.md.
NON_TERMINAL_STATUSES = (
    AgentRunStatus.QUEUED,
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.RUNNING,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
    AgentRunStatus.PAUSED_AWAITING_INPUT,
)

# Statuses that indicate a runner is currently serving a run.
BUSY_STATUSES = (
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.RUNNING,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
)


# ---------------------------------------------------------------------------
# Pod-scoped helpers (new, used by Phase 3 and beyond)
# ---------------------------------------------------------------------------


def select_runner_in_pod(pod: Pod) -> Optional[Runner]:
    """Pick an online, heartbeat-fresh, idle runner inside ``pod``.

    Must be called inside a ``transaction.atomic()`` block — the query takes a
    row-level lock (``SELECT … FOR UPDATE SKIP LOCKED``) so concurrent drains
    can't both pick the same runner.
    """
    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    return (
        Runner.objects.select_for_update(skip_locked=True)
        .filter(
            pod=pod,
            status=RunnerStatus.ONLINE,
            last_heartbeat_at__gte=alive_threshold,
        )
        .exclude(agent_runs__status__in=BUSY_STATUSES)
        .order_by("-last_heartbeat_at")
        .first()
    )


def next_queued_run_for_pod(pod: Pod) -> Optional[AgentRun]:
    """Return the oldest unpinned QUEUED run in the pod, locked for update.

    Must be called inside a ``transaction.atomic()`` block.

    Pinned runs (``pinned_runner_id IS NOT NULL``) are excluded — they belong
    to a specific runner and are served via :func:`next_for_runner`. This
    keeps the legacy run-first matcher honest under the new pinning model.
    """
    return (
        AgentRun.objects.select_for_update(skip_locked=True)
        .filter(
            pod=pod,
            status=AgentRunStatus.QUEUED,
            pinned_runner__isnull=True,
        )
        .order_by("created_at")
        .first()
    )


def next_for_runner(runner: Runner) -> Optional[AgentRun]:
    """Return the next QUEUED run this runner should take.

    Personal queue first (runs pinned to this runner), then the pod
    general queue (unpinned runs in the same pod). FIFO by ``created_at``
    inside each queue. Returns the run row locked
    ``FOR UPDATE SKIP LOCKED`` so concurrent drains don't double-assign.

    Must be called inside a ``transaction.atomic()`` block.
    """
    return (
        AgentRun.objects.select_for_update(skip_locked=True)
        .filter(
            pod=runner.pod,
            status=AgentRunStatus.QUEUED,
        )
        .filter(
            Q(pinned_runner=runner) | Q(pinned_runner__isnull=True),
        )
        # Pinned-to-me sorts before unpinned thanks to NULLS LAST on the
        # boolean expression. ``BooleanField`` annotated to give us a
        # stable, indexable sort key without requiring a CASE WHEN.
        .annotate(_is_mine=models.Case(
            models.When(pinned_runner=runner, then=models.Value(0)),
            default=models.Value(1),
            output_field=models.IntegerField(),
        ))
        .order_by("_is_mine", "created_at")
        .first()
    )


def drain_pod(pod: Pod) -> int:
    """Assign as many QUEUED runs in ``pod`` to idle runners as possible.

    Returns the number of runs assigned in this pass. The loop is
    runner-first: for each idle runner, pick the best run for it
    (personal queue > pod queue). This eliminates head-of-line blocking
    when the head QUEUED run is pinned to a busy runner — other idle
    runners can still serve unpinned work.

    Dispatch messages are sent over the WebSocket ``on_commit`` so
    receivers never observe a run that hasn't landed in the DB.
    """
    from pi_dash.runner.services.pubsub import send_to_runner

    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    assignments: list[tuple[Runner, AgentRun]] = []
    with transaction.atomic():
        idle_runners = list(
            Runner.objects.select_for_update(skip_locked=True)
            .filter(
                pod=pod,
                status=RunnerStatus.ONLINE,
                last_heartbeat_at__gte=alive_threshold,
            )
            .exclude(agent_runs__status__in=BUSY_STATUSES)
            .order_by("-last_heartbeat_at")
        )
        for runner in idle_runners:
            run = next_for_runner(runner)
            if run is None:
                continue
            run.runner = runner
            run.owner_id = runner.owner_id
            run.status = AgentRunStatus.ASSIGNED
            run.assigned_at = timezone.now()
            run.save(update_fields=["runner", "owner", "status", "assigned_at"])
            assignments.append((runner, run))

    for runner, run in assignments:
        transaction.on_commit(
            lambda r=run, rn=runner: send_to_runner(rn.id, _build_assign_msg(r))
        )
    if assignments:
        logger.info(
            "drain_pod: pod=%s assigned %d run(s)", pod.id, len(assignments)
        )
    return len(assignments)


def drain_pod_by_id(pod_id) -> int:
    """Convenience wrapper for callers that only have a pod id.

    Skips if the pod has been soft-deleted.
    """
    pod = Pod.objects.filter(pk=pod_id).first()
    if pod is None:
        return 0
    return drain_pod(pod)


def drain_for_runner(runner: Runner) -> bool:
    """Try to assign one QUEUED run to ``runner``.

    Used as the immediate-dispatch trigger when a runner becomes idle
    (a run terminates or pauses) or reconnects with pinned work waiting.
    Returns True if a run was assigned.
    """
    from pi_dash.runner.services.pubsub import send_to_runner

    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    assignment: Optional[tuple[Runner, AgentRun]] = None
    with transaction.atomic():
        # Re-select the runner under lock so two concurrent drain calls
        # for the same runner can't both assign it.
        locked = (
            Runner.objects.select_for_update(skip_locked=True)
            .filter(
                pk=runner.pk,
                status=RunnerStatus.ONLINE,
                last_heartbeat_at__gte=alive_threshold,
            )
            .exclude(agent_runs__status__in=BUSY_STATUSES)
            .first()
        )
        if locked is None:
            return False
        run = next_for_runner(locked)
        if run is None:
            return False
        run.runner = locked
        run.owner_id = locked.owner_id
        run.status = AgentRunStatus.ASSIGNED
        run.assigned_at = timezone.now()
        run.save(update_fields=["runner", "owner", "status", "assigned_at"])
        assignment = (locked, run)

    runner_obj, run = assignment
    transaction.on_commit(
        lambda r=run, rn=runner_obj: send_to_runner(rn.id, _build_assign_msg(r))
    )
    logger.info(
        "drain_for_runner: runner=%s assigned run=%s", runner_obj.id, run.id
    )
    return True


def drain_for_runner_by_id(runner_id) -> bool:
    """Convenience wrapper for callers that only have a runner id."""
    runner = Runner.objects.filter(pk=runner_id).first()
    if runner is None:
        return False
    return drain_for_runner(runner)


def _build_assign_msg(run: AgentRun) -> dict:
    """Compose the WS ``assign`` envelope sent to a runner daemon.

    Mirrors the shape currently inlined in ``runs.py`` POST and
    ``orchestration/service._dispatch_to_runner``; Phase 3 will point both at
    this helper.
    """
    # resume_thread_id is set when this run is a continuation of a prior
    # run that recorded a session id (Codex thread_id / Claude session_id).
    # The runner uses it to call thread/resume or claude --resume so the
    # agent re-attaches to its prior in-memory state. Empty/missing on the
    # wire means "fresh session" — backward compatible with older runners.
    parent = run.parent_run
    resume_thread_id = parent.thread_id if (parent and parent.thread_id) else None
    return {
        "v": 1,
        "type": "assign",
        "run_id": str(run.id),
        "work_item_id": str(run.work_item_id) if run.work_item_id else None,
        "prompt": run.prompt,
        "repo_url": run.run_config.get("repo_url"),
        "repo_ref": run.run_config.get("repo_ref"),
        "git_work_branch": run.run_config.get("git_work_branch"),
        "expected_codex_model": run.run_config.get("model"),
        "approval_policy_overrides": run.run_config.get("approval_policy_overrides"),
        "deadline": None,
        "resume_thread_id": resume_thread_id,
    }


# ---------------------------------------------------------------------------
# Legacy helpers (kept until Phase 3 removes callers)
# ---------------------------------------------------------------------------


def select_runner_for_run(run: AgentRun) -> Optional[Runner]:
    """Legacy owner-scoped matcher. Kept for back-compat.

    Under the pod model, use :func:`drain_pod` or :func:`select_runner_in_pod`
    instead. This function filters by ``owner=run.owner``, which was the old
    access-gating rule. Phase 3 migrates callers.
    """
    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    return (
        Runner.objects.select_for_update(skip_locked=True)
        .filter(
            owner=run.owner,
            workspace=run.workspace,
            status=RunnerStatus.ONLINE,
            last_heartbeat_at__gte=alive_threshold,
        )
        .exclude(agent_runs__status__in=BUSY_STATUSES)
        .order_by("-last_heartbeat_at")
        .first()
    )


def can_register_another(user_id, workspace_id) -> bool:
    """Enforce the per-user runner cap."""
    active = Runner.objects.filter(
        owner_id=user_id,
        workspace_id=workspace_id,
    ).exclude(status=RunnerStatus.REVOKED)
    return active.count() < Runner.MAX_PER_USER


def count_active(user_id, workspace_id) -> int:
    return (
        Runner.objects.filter(owner_id=user_id, workspace_id=workspace_id)
        .exclude(status=RunnerStatus.REVOKED)
        .count()
    )


def eligible_for_assignment(runner: Runner) -> Q:
    """Convenience predicate for assertions/tests."""
    return Q(pk=runner.pk, status=RunnerStatus.ONLINE)
