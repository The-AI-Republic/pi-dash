"""Celery lifecycle for the stateless Cloud Agent executor."""

from __future__ import annotations

import asyncio
import json
import logging
import random
from datetime import timedelta

from celery import shared_task
from celery.exceptions import SoftTimeLimitExceeded
from django.conf import settings
from django.db import transaction
from django.db import close_old_connections
from django.db.models import Min, Q
from django.utils import timezone

from pi_dash.cloud_agent import events
from pi_dash.core.agent_execution import AgentExecutorKind, cloud_agent_is_configured
from pi_dash.core.permissions import ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, check_project_role, is_workspace_member
from pi_dash.db.models import Workspace
from pi_dash.runner.models import AgentRun, AgentRunStatus
from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

logger = logging.getLogger(__name__)


def _claim(run_id):
    with transaction.atomic():
        run = (
            AgentRun.objects.select_for_update()
            .select_related("created_by")
            .filter(pk=run_id, executor_kind=AgentExecutorKind.CLOUD_AGENT, status=AgentRunStatus.QUEUED)
            .first()
        )
        if run is None:
            return None
        Workspace.objects.select_for_update().get(pk=run.workspace_id)
        running = AgentRun.objects.filter(
            workspace_id=run.workspace_id,
            executor_kind=AgentExecutorKind.CLOUD_AGENT,
            status=AgentRunStatus.RUNNING,
        ).count()
        if running >= settings.CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE:
            backoff = settings.CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS
            run.lease_expires_at = timezone.now() + timedelta(
                seconds=random.randint(max(1, backoff // 2), max(1, backoff + backoff // 2))
            )
            run.save(update_fields=["lease_expires_at"])
            return None
        run.status = AgentRunStatus.RUNNING
        run.started_at = timezone.now()
        run.lease_expires_at = None
        run.save(update_fields=["status", "started_at", "lease_expires_at"])
    events.append(run.id, "run_started", {})
    return AgentRun.objects.select_related(
        "created_by",
        "workspace",
        "work_item",
        "work_item__project",
        "scheduler_binding__project",
        "pod__project",
    ).get(pk=run.id)


def _fail(run_id, code, detail):
    finalize_agent_run(
        run_id, AgentRunStatus.FAILED, updates={"error_code": code[:64], "error": (detail or code)[:16000]}
    )


@shared_task(
    name="cloud_agent.run_agent_run",
    acks_late=False,
    max_retries=0,
    soft_time_limit=settings.CLOUD_AGENT_RUN_SOFT_LIMIT_SECONDS,
    time_limit=settings.CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS,
)
def run_cloud_agent(run_id):
    close_old_connections()
    run = _claim(run_id)
    if run is None:
        close_old_connections()
        return "ignored"
    if not cloud_agent_is_configured():
        _fail(run.id, "cloud_agent_disabled", "Pi Dash Cloud Agent is disabled or not configured")
        close_old_connections()
        return "disabled"
    if (
        not run.created_by.is_active
        or run.created_by.is_bot
        or not is_workspace_member(run.created_by, run.workspace_id)
    ):
        _fail(run.id, "actor_no_longer_authorized", "The initiating user no longer belongs to this workspace")
        close_old_connections()
        return "unauthorized"
    project = (
        run.work_item.project
        if run.work_item_id
        else run.scheduler_binding.project
        if run.scheduler_binding_id
        else run.pod.project
    )
    if not check_project_role(
        run.created_by,
        run.workspace.slug,
        project.id,
        [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
    ):
        _fail(run.id, "actor_no_longer_authorized", "The initiating user no longer belongs to this project")
        close_old_connections()
        return "unauthorized"
    if len(run.prompt.encode()) > settings.CLOUD_AGENT_MAX_PROMPT_BYTES:
        _fail(run.id, "prompt_too_large", "The composed Cloud Agent prompt exceeds the configured limit")
        close_old_connections()
        return "prompt_too_large"
    if run.cancel_requested_at:
        finalize_agent_run(
            run.id, AgentRunStatus.CANCELLED, updates={"error_code": "cancelled", "error": run.cancel_reason}
        )
        close_old_connections()
        return "cancelled"
    try:
        from pi_dash.cloud_agent.policy import resolve_current_tool_names

        current_tools = resolve_current_tool_names(run)
        run.tool_plan = {**run.tool_plan, "tools": current_tools}
        output, usage = asyncio.run(__import__("pi_dash.cloud_agent.runtime", fromlist=["execute"]).execute(run))
        run.refresh_from_db(fields=["cancel_requested_at", "cancel_reason"])
        if not cloud_agent_is_configured():
            _fail(run.id, "cloud_agent_disabled", "Pi Dash Cloud Agent was disabled during execution")
            close_old_connections()
            return "disabled"
        if run.cancel_requested_at:
            finalize_agent_run(
                run.id, AgentRunStatus.CANCELLED, updates={"error_code": "cancelled", "error": run.cancel_reason}
            )
            close_old_connections()
            return "cancelled"
        tool_calls = run.tool_calls.filter(status="succeeded").count()
        writes = run.tool_calls.filter(status="succeeded", risk="write").count()
        payload = {
            "v": 1,
            "executor": "cloud_agent",
            "status": output.outcome,
            "summary": output.summary,
            "evidence": output.evidence,
            "tool_calls": tool_calls,
            "writes": writes,
            "limitations": output.limitations,
        }
        if (
            len(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode())
            > settings.CLOUD_AGENT_MAX_FINAL_RESULT_BYTES
        ):
            _fail(run.id, "final_result_too_large", "The structured result exceeds the configured limit")
            close_old_connections()
            return "result_too_large"
        finalize_agent_run(
            run.id,
            AgentRunStatus.BLOCKED if output.outcome == "blocked" else AgentRunStatus.COMPLETED,
            updates={
                "done_payload": payload,
                "error": "",
                "error_code": "",
                "llm_model": settings.CLOUD_AGENT_MODEL,
                **usage,
            },
        )
        close_old_connections()
        return "completed"
    except SoftTimeLimitExceeded:
        _fail(run.id, "run_timeout", "Cloud Agent execution exceeded its time limit")
    except TimeoutError:
        _fail(run.id, "run_timeout", "Cloud Agent execution exceeded its time limit")
    except Exception as exc:
        from pi_dash.cloud_agent.errors import classify_error

        code, text = classify_error(exc)
        logger.exception("Cloud Agent run %s failed", run.id)
        if code == "provider_refusal":
            finalize_agent_run(
                run.id,
                AgentRunStatus.REFUSED,
                updates={
                    "error_code": code,
                    "error": text,
                    "refusal_category": "unknown",
                },
            )
        else:
            _fail(run.id, getattr(exc, "code", code), text)
    close_old_connections()
    return "failed"


@shared_task(name="cloud_agent.scan_queued_runs")
def scan_queued_runs():
    now = timezone.now()
    max_age = now - timedelta(seconds=settings.CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS)
    expired = AgentRun.objects.filter(
        executor_kind=AgentExecutorKind.CLOUD_AGENT, status=AgentRunStatus.QUEUED, created_at__lt=max_age
    )
    for run_id in list(expired.values_list("id", flat=True)[: settings.CLOUD_AGENT_DISPATCH_SCAN_BATCH]):
        _fail(run_id, "dispatch_timeout", "Cloud Agent run exceeded the maximum queue age")
    workspace_ids = list(
        AgentRun.objects.filter(executor_kind=AgentExecutorKind.CLOUD_AGENT, status=AgentRunStatus.QUEUED)
        .filter(Q(lease_expires_at__isnull=True) | Q(lease_expires_at__lte=now))
        .values("workspace_id")
        .annotate(oldest_queued_at=Min("created_at"))
        .order_by("oldest_queued_at")
        .values_list("workspace_id", flat=True)[: settings.CLOUD_AGENT_DISPATCH_SCAN_BATCH]
    )
    if not settings.CLOUD_AGENT_ENABLED:
        for run_id in AgentRun.objects.filter(
            executor_kind=AgentExecutorKind.CLOUD_AGENT, status=AgentRunStatus.QUEUED
        ).values_list("id", flat=True)[: settings.CLOUD_AGENT_DISPATCH_SCAN_BATCH]:
            _fail(run_id, "cloud_agent_disabled", "Pi Dash Cloud Agent is disabled")
        return 0
    from pi_dash.cloud_agent.dispatch import dispatch_waiting

    return sum(dispatch_waiting(workspace_id) for workspace_id in workspace_ids)


@shared_task(name="cloud_agent.sweep_stale_runs")
def sweep_stale_runs():
    cutoff = timezone.now() - timedelta(
        seconds=settings.CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS + settings.CLOUD_AGENT_STALE_GRACE_SECONDS
    )
    runs = AgentRun.objects.filter(
        executor_kind=AgentExecutorKind.CLOUD_AGENT, status=AgentRunStatus.RUNNING, started_at__lt=cutoff
    )[: settings.CLOUD_AGENT_DISPATCH_SCAN_BATCH]
    count = 0
    for run in runs:
        if run.cancel_requested_at:
            if finalize_agent_run(
                run.id, AgentRunStatus.CANCELLED, updates={"error_code": "cancelled", "error": run.cancel_reason}
            ):
                count += 1
        elif finalize_agent_run(
            run.id,
            AgentRunStatus.FAILED,
            updates={"error_code": "run_timeout", "error": "Cloud Agent worker was lost or exceeded its deadline"},
        ):
            count += 1
    return count
