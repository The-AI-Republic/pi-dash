# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prometheus text-format metrics endpoint.

Exposes point-in-time gauges derived from DB queries. Scrape with
``GET /api/v1/runner/metrics/``. No new dependencies — we emit the format
ourselves so the endpoint works with any Prometheus-compatible scraper.

Schema (all gauges):
- ``apple_pi_dash_runner_online``: count of runners in ``online`` state.
- ``apple_pi_dash_runner_busy``: count of runners in ``busy`` state.
- ``apple_pi_dash_runner_offline``: count of runners in ``offline`` state
  (excludes revoked; revoked runners are retired, not a health signal).
- ``apple_pi_dash_runs_active``: count of AgentRuns in an in-flight status
  (assigned / running / awaiting_approval / awaiting_reauth).
- ``apple_pi_dash_approvals_pending``: count of ApprovalRequests with
  ``status=pending``.
"""

from __future__ import annotations

from django.db.models import Count
from django.http import HttpResponse
from rest_framework.permissions import AllowAny
from rest_framework.views import APIView

from apple_pi_dash.runner.models import (
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
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
)


def _gauge(name: str, help_text: str, value: int) -> str:
    return (
        f"# HELP {name} {help_text}\n"
        f"# TYPE {name} gauge\n"
        f"{name} {value}\n"
    )


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

        body = "".join([
            _gauge(
                "apple_pi_dash_runner_online",
                "Runners currently online.",
                online,
            ),
            _gauge(
                "apple_pi_dash_runner_busy",
                "Runners currently executing a run.",
                busy,
            ),
            _gauge(
                "apple_pi_dash_runner_offline",
                "Runners that have dropped their heartbeat (excludes revoked).",
                offline,
            ),
            _gauge(
                "apple_pi_dash_runs_active",
                "AgentRuns in an in-flight status.",
                active_runs,
            ),
            _gauge(
                "apple_pi_dash_approvals_pending",
                "ApprovalRequests waiting for a decision.",
                pending_approvals,
            ),
        ])
        return HttpResponse(body, content_type="text/plain; version=0.0.4")
