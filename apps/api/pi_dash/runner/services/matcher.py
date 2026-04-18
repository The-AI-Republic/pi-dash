# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner selection for an AgentRun.

MVP rule (per ``.ai_design/implement_runner/runner-design.md``): pick one
online idle runner owned by the user. No label matching. Users may register
up to ``Runner.MAX_PER_USER`` runners; the matcher still needs to pick only
one per run.
"""

from __future__ import annotations

from datetime import timedelta
from typing import Optional

from django.db.models import Q
from django.utils import timezone

from pi_dash.runner.models import AgentRun, AgentRunStatus, Runner, RunnerStatus

# A runner is considered stale (no longer eligible) if it has not heartbeated
# within this window. Defensive in case a runner crashed without calling Bye.
HEARTBEAT_GRACE = timedelta(seconds=90)


def select_runner_for_run(run: AgentRun) -> Optional[Runner]:
    """Return a suitable Runner or ``None`` if no machine is currently eligible.

    Caller must be inside a ``transaction.atomic()`` block — the query takes a
    row-level lock (``SELECT … FOR UPDATE SKIP LOCKED``) so two concurrent
    assignments can't pick the same runner.
    """
    alive_threshold = timezone.now() - HEARTBEAT_GRACE
    busy_states = (
        AgentRunStatus.ASSIGNED,
        AgentRunStatus.RUNNING,
        AgentRunStatus.AWAITING_APPROVAL,
        AgentRunStatus.AWAITING_REAUTH,
    )
    return (
        Runner.objects.select_for_update(skip_locked=True)
        .filter(
            owner=run.owner,
            workspace=run.workspace,
            status=RunnerStatus.ONLINE,
            last_heartbeat_at__gte=alive_threshold,
        )
        .exclude(agent_runs__status__in=busy_states)
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
