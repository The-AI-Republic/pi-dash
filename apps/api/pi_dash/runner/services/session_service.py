# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared session-open / hello-apply service.

Carved out of ``consumers.py`` per ``.ai_design/move_to_https/tasks.md``
§2.1 so both the legacy WS Hello path and the new
``POST /runners/<rid>/sessions/`` endpoint apply the same logic.
"""

from __future__ import annotations

import logging
from datetime import datetime, timedelta
from typing import Any, Dict, Optional
from uuid import UUID

from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerStatus,
)

logger = logging.getLogger(__name__)

OFFLINE_GRACE_SECS = 60


def apply_hello(runner: Runner, body: Dict[str, Any]) -> None:
    """Update runner metadata + reap stale busy runs.

    ``body`` is the session-open / Hello payload. Persists ``os``,
    ``arch``, ``version``, and bumps ``last_heartbeat_at``.
    """
    runner.os = body.get("os", "") or runner.os
    runner.arch = body.get("arch", "") or runner.arch
    runner.runner_version = body.get("version", "") or runner.runner_version
    runner.last_heartbeat_at = timezone.now()
    runner.save(
        update_fields=["os", "arch", "runner_version", "last_heartbeat_at"]
    )
    reap_stale_busy_runs(runner, body)


def reap_stale_busy_runs(runner: Runner, body: Dict[str, Any]) -> None:
    """Cancel BUSY runs the daemon no longer claims."""
    from django.db import transaction

    from pi_dash.runner.services.matcher import (
        BUSY_STATUSES,
        drain_for_runner_by_id,
        drain_pod_by_id,
    )

    now = timezone.now()
    ts_raw = body.get("ts")
    try:
        heartbeat_ts = (
            datetime.fromisoformat(ts_raw.replace("Z", "+00:00"))
            if isinstance(ts_raw, str)
            else now
        )
    except (ValueError, AttributeError):
        heartbeat_ts = now
    heartbeat_ts = min(heartbeat_ts, now)
    heartbeat_ts = max(heartbeat_ts, now - timedelta(seconds=OFFLINE_GRACE_SECS))

    in_flight = body.get("in_flight_run")
    in_flight_id: Optional[str] = None
    if in_flight:
        try:
            in_flight_id = str(UUID(str(in_flight)))
        except (ValueError, AttributeError):
            in_flight_id = None

    stale = AgentRun.objects.filter(
        runner=runner,
        status__in=BUSY_STATUSES,
        assigned_at__lt=heartbeat_ts,
    )
    if in_flight_id:
        stale = stale.exclude(id=in_flight_id)
    reaped = list(stale.values_list("id", "pod_id"))
    if not reaped:
        return

    AgentRun.objects.filter(id__in=[rid for rid, _ in reaped]).update(
        status=AgentRunStatus.FAILED,
        ended_at=now,
        error=(
            "reaped by heartbeat: runner reported in_flight_run="
            f"{in_flight_id or '(none)'} but cloud had this run marked busy"
        ),
    )
    pod_ids = {pid for _, pid in reaped if pid is not None}
    runner_id = runner.id

    def _drain_after_commit(rid=runner_id, pids=pod_ids):
        drain_for_runner_by_id(rid)
        for pid in pids:
            drain_pod_by_id(pid)

    transaction.on_commit(_drain_after_commit)


def mark_runner_online(runner_id: UUID | str) -> None:
    Runner.objects.filter(pk=runner_id).update(
        status=RunnerStatus.ONLINE, last_heartbeat_at=timezone.now()
    )


def mark_runner_offline(runner_id: UUID | str) -> None:
    Runner.objects.filter(pk=runner_id).exclude(
        status=RunnerStatus.REVOKED
    ).update(status=RunnerStatus.OFFLINE)


def resolve_runner_project_slug(runner: Runner) -> Optional[str]:
    """Return ``runner.pod.project.identifier`` or ``None``."""
    r = (
        Runner.objects.select_related("pod__project")
        .filter(pk=runner.pk)
        .first()
    )
    if r is None or r.pod_id is None:
        return None
    project = r.pod.project
    if project is None:
        return None
    return project.identifier


def build_resume_ack(runner: Runner, run_id: str) -> Optional[Dict[str, Any]]:
    """Return a ``resume_ack`` payload for an in-flight run, or ``None``.

    See ``consumers.RunnerConsumer._resume_run``: if the run does not
    exist, send a ``cancel`` instead; if it has terminated, send
    ``cancel`` with the terminal status.
    """
    from pi_dash.runner.models import AgentRunEvent

    run = AgentRun.objects.filter(id=run_id, runner=runner).first()
    if run is None:
        return {
            "type": "cancel",
            "run_id": str(run_id),
            "reason": "unknown_run_on_reconnect",
        }
    if run.is_terminal:
        return {
            "type": "cancel",
            "run_id": str(run_id),
            "reason": f"run_already_{run.status}",
        }
    last_seq = (
        AgentRunEvent.objects.filter(agent_run_id=run_id)
        .order_by("-seq")
        .values_list("seq", flat=True)
        .first()
    )
    return {
        "type": "resume_ack",
        "run_id": str(run_id),
        "last_seq": last_seq,
        "status": run.status,
        "thread_id": run.thread_id,
    }
