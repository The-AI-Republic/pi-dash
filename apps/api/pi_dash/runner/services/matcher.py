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

from django.db import transaction
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
NON_TERMINAL_STATUSES = (
    AgentRunStatus.QUEUED,
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.RUNNING,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
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
    """Return the oldest QUEUED run in the pod, locked for update.

    Must be called inside a ``transaction.atomic()`` block.
    """
    return (
        AgentRun.objects.select_for_update(skip_locked=True)
        .filter(pod=pod, status=AgentRunStatus.QUEUED)
        .order_by("created_at")
        .first()
    )


def drain_pod(pod: Pod) -> int:
    """Assign as many QUEUED runs in ``pod`` to idle runners as possible.

    Returns the number of runs assigned in this pass. Dispatch messages are
    sent over the WebSocket ``on_commit`` so receivers never see a run that
    hasn't landed in the DB.
    """
    # Late import to avoid an import cycle (pubsub imports matcher in no path
    # I can see today, but this keeps future refactors safe).
    from pi_dash.runner.services.pubsub import send_to_runner

    assignments: list[tuple[Runner, AgentRun]] = []
    with transaction.atomic():
        while True:
            run = next_queued_run_for_pod(pod)
            if run is None:
                break
            runner = select_runner_in_pod(pod)
            if runner is None:
                break
            run.runner = runner
            # Capture billable party at assignment (design §5.3).
            run.owner_id = runner.owner_id
            run.status = AgentRunStatus.ASSIGNED
            run.assigned_at = timezone.now()
            run.save(update_fields=["runner", "owner", "status", "assigned_at"])
            assignments.append((runner, run))

    # Fire WS dispatches after the transaction commits.
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


def _build_assign_msg(run: AgentRun) -> dict:
    """Compose the WS ``assign`` envelope sent to a runner daemon.

    Mirrors the shape currently inlined in ``runs.py`` POST and
    ``orchestration/service._dispatch_to_runner``; Phase 3 will point both at
    this helper.
    """
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
