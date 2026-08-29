"""Database-backed Cloud Agent admission and dispatch."""

from __future__ import annotations

import logging
from datetime import timedelta

from django.conf import settings
from django.db import transaction
from django.db.models import F, Q
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.db.models import Workspace
from pi_dash.runner.models import AgentRun, AgentRunStatus

logger = logging.getLogger(__name__)


def dispatch_waiting(workspace_id) -> int:
    """Lease and offer oldest queued rows up to workspace capacity."""
    if not settings.CLOUD_AGENT_ENABLED:
        return 0
    now = timezone.now()
    ids = []
    with transaction.atomic():
        Workspace.objects.select_for_update().get(pk=workspace_id)
        base = AgentRun.objects.filter(executor_kind=AgentExecutorKind.CLOUD_AGENT, workspace_id=workspace_id)
        running = base.filter(status=AgentRunStatus.RUNNING).count()
        offered = base.filter(status=AgentRunStatus.QUEUED, lease_expires_at__gt=now).count()
        capacity = max(0, settings.CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE - running - offered)
        if not capacity:
            return 0
        rows = list(
            base.select_for_update(skip_locked=True)
            .filter(status=AgentRunStatus.QUEUED)
            .filter(Q(lease_expires_at__isnull=True) | Q(lease_expires_at__lte=now))
            .order_by("created_at")[:capacity]
        )
        lease = now + timedelta(seconds=settings.CLOUD_AGENT_DISPATCH_LEASE_SECONDS)
        for run in rows:
            AgentRun.objects.filter(pk=run.pk, status=AgentRunStatus.QUEUED).update(
                lease_expires_at=lease, dispatch_attempts=F("dispatch_attempts") + 1
            )
            ids.append(str(run.pk))
        if ids:
            transaction.on_commit(lambda: _publish(ids))
    return len(ids)


def _publish(run_ids):
    from pi_dash.cloud_agent.tasks import run_cloud_agent

    for run_id in run_ids:
        try:
            run_cloud_agent.delay(run_id)
        except Exception:
            logger.exception("Cloud Agent broker publication failed for run %s", run_id)


def dispatch_agent_run(run_id) -> None:
    run = AgentRun.objects.only("id", "status", "executor_kind", "pod_id", "workspace_id").filter(pk=run_id).first()
    if run is None or run.status != AgentRunStatus.QUEUED:
        return
    if run.executor_kind == AgentExecutorKind.CLOUD_AGENT:
        dispatch_waiting(run.workspace_id)
    else:
        from pi_dash.runner.services.matcher import drain_pod_by_id

        drain_pod_by_id(run.pod_id)
