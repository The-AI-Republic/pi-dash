# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prometheus text-format metrics endpoint.

Exposes point-in-time gauges derived from DB queries. Scrape with
``GET /api/v1/runner/metrics/``. No new dependencies — we emit the format
ourselves so the endpoint works with any Prometheus-compatible scraper.

Schema (all gauges):
- ``pi_dash_runner_online``: count of runners in ``online`` state.
- ``pi_dash_runner_busy``: count of runners in ``busy`` state.
- ``pi_dash_runner_offline``: count of runners in ``offline`` state
  (excludes revoked; revoked runners are retired, not a health signal).
- ``pi_dash_runs_active``: count of AgentRuns in an in-flight status
  (assigned / running / cancel_requested / awaiting_approval /
  awaiting_reauth).
- ``pi_dash_approvals_pending``: count of ApprovalRequests with
  ``status=pending``.
- ``pi_dash_runs_failed_by_reason{reason="…"}``: count of FAILED
  AgentRuns per canonical ``failure_reason`` (PDASHOSS01-183). Rows that
  predate the taxonomy backfill (blank reason) are omitted.
"""

from __future__ import annotations

from django.db.models import Count
from django.http import HttpResponse
from rest_framework.permissions import AllowAny
from rest_framework.views import APIView

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
    Runner,
    RunnerStatus,
)


ACTIVE_RUN_STATUSES = (
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.RUNNING,
    AgentRunStatus.CANCEL_REQUESTED,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
)


def _gauge(name: str, help_text: str, value: int) -> str:
    return (
        f"# HELP {name} {help_text}\n"
        f"# TYPE {name} gauge\n"
        f"{name} {value}\n"
    )


def _labeled_gauge(name: str, help_text: str, rows: list[tuple[str, str, int]]) -> str:
    """One gauge family with a single label. ``rows`` is
    ``[(label_key, label_value, value), …]``; label values are escaped per
    the Prometheus text format."""
    lines = [f"# HELP {name} {help_text}", f"# TYPE {name} gauge"]
    for key, value, count in rows:
        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        lines.append(f'{name}{{{key}="{escaped}"}} {count}')
    return "\n".join(lines) + "\n"


class MetricsEndpoint(APIView):
    authentication_classes: list = []
    permission_classes = [AllowAny]
    # Metrics scrapers expect ``text/plain``; DRF defaults to JSON. We bypass
    # DRF rendering by returning ``HttpResponse`` directly.

    def get(self, request):
        status_counts = dict(
            Runner.objects.values_list("status").annotate(c=Count("id"))
        )
        online = status_counts.get(RunnerStatus.ONLINE, 0)
        busy = status_counts.get(RunnerStatus.BUSY, 0)
        offline = status_counts.get(RunnerStatus.OFFLINE, 0)

        active_runs = AgentRun.objects.filter(
            status__in=ACTIVE_RUN_STATUSES
        ).count()
        pending_approvals = ApprovalRequest.objects.filter(
            status=ApprovalStatus.PENDING
        ).count()
        failed_by_reason = sorted(
            AgentRun.objects.filter(status=AgentRunStatus.FAILED)
            .exclude(failure_reason="")
            .values_list("failure_reason")
            .annotate(c=Count("id"))
        )

        body = "".join([
            _gauge(
                "pi_dash_runner_online",
                "Runners currently online.",
                online,
            ),
            _gauge(
                "pi_dash_runner_busy",
                "Runners currently executing a run.",
                busy,
            ),
            _gauge(
                "pi_dash_runner_offline",
                "Runners that have dropped their heartbeat (excludes revoked).",
                offline,
            ),
            _gauge(
                "pi_dash_runs_active",
                "AgentRuns in an in-flight status.",
                active_runs,
            ),
            _gauge(
                "pi_dash_approvals_pending",
                "ApprovalRequests waiting for a decision.",
                pending_approvals,
            ),
            _labeled_gauge(
                "pi_dash_runs_failed_by_reason",
                "FAILED AgentRuns per canonical failure_reason.",
                [("reason", reason, count) for reason, count in failed_by_reason],
            ),
        ])
        return HttpResponse(body, content_type="text/plain; version=0.0.4")
