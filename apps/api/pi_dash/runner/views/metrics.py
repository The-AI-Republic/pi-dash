# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prometheus text-format metrics endpoint.

Exposes point-in-time series derived from DB queries. Scrape with
``GET /api/v1/runner/metrics/``. No new dependencies — we emit the format
ourselves so the endpoint works with any Prometheus-compatible scraper, and
every series is a snapshot computed at scrape time rather than an in-process
registry that would not survive a worker restart.

Schema:
- ``pi_dash_runner_online`` (gauge): count of runners in ``online`` state.
- ``pi_dash_runner_busy`` (gauge): count of runners in ``busy`` state.
- ``pi_dash_runner_offline`` (gauge): count of runners in ``offline`` state
  (excludes revoked; revoked runners are retired, not a health signal).
- ``pi_dash_managed_runner_online`` (gauge): online desktop-bundled (managed)
  runners — a subset of ``pi_dash_runner_online`` scoped to the managed kind
  (design §15.1).
- ``pi_dash_runs_active`` (gauge): count of AgentRuns in an in-flight status
  (assigned / running / cancel_requested / awaiting_approval /
  awaiting_reauth).
- ``pi_dash_runs_total`` (counter, label ``executor_kind``): AgentRuns ever
  created, one series per executor kind. DB rows are never deleted in normal
  operation, so the DB count is monotonic and reads as a counter (§15.1).
- ``pi_dash_managed_runner_queued_wait_seconds`` (histogram): age of managed
  runs currently ``QUEUED`` (waiting for a closed desktop), so the distribution
  of how long managed work has been parked is visible (§8.5, §15.1).
- ``pi_dash_approvals_pending`` (gauge): count of ApprovalRequests with
  ``status=pending``.
"""

from __future__ import annotations

from django.db.models import Count
from django.http import HttpResponse
from django.utils import timezone
from rest_framework.permissions import AllowAny
from rest_framework.views import APIView

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
    Runner,
    RunnerProvisioning,
    RunnerStatus,
)


ACTIVE_RUN_STATUSES = (
    AgentRunStatus.ASSIGNED,
    AgentRunStatus.RUNNING,
    AgentRunStatus.CANCEL_REQUESTED,
    AgentRunStatus.AWAITING_APPROVAL,
    AgentRunStatus.AWAITING_REAUTH,
)

# Upper edges (seconds) for the managed queued-wait histogram: 1m, 5m, 15m, 1h,
# 3h, 6h, 12h. The last finite bucket matches the default
# ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`` (12h) so a run about to be swept lands
# in the top finite bucket rather than only in ``+Inf``.
QUEUED_WAIT_BUCKETS_SECONDS = (60, 300, 900, 3600, 10800, 21600, 43200)


def _gauge(name: str, help_text: str, value: int) -> str:
    return (
        f"# HELP {name} {help_text}\n"
        f"# TYPE {name} gauge\n"
        f"{name} {value}\n"
    )


def _counter(name: str, help_text: str, samples: list[tuple[str, int]]) -> str:
    """A counter with a single ``executor_kind`` label per sample."""
    lines = [f"# HELP {name} {help_text}", f"# TYPE {name} counter"]
    for label_value, value in samples:
        lines.append(f'{name}{{executor_kind="{label_value}"}} {value}')
    return "\n".join(lines) + "\n"


def _histogram(name: str, help_text: str, buckets: tuple[int, ...], observations: list[float]) -> str:
    """Cumulative-bucket histogram in Prometheus text format."""
    lines = [f"# HELP {name} {help_text}", f"# TYPE {name} histogram"]
    for edge in buckets:
        count = sum(1 for obs in observations if obs <= edge)
        lines.append(f'{name}_bucket{{le="{edge}"}} {count}')
    lines.append(f'{name}_bucket{{le="+Inf"}} {len(observations)}')
    lines.append(f"{name}_sum {sum(observations)}")
    lines.append(f"{name}_count {len(observations)}")
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

        managed_online = Runner.objects.filter(
            provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
            status=RunnerStatus.ONLINE,
        ).count()

        active_runs = AgentRun.objects.filter(
            status__in=ACTIVE_RUN_STATUSES
        ).count()
        pending_approvals = ApprovalRequest.objects.filter(
            status=ApprovalStatus.PENDING
        ).count()

        runs_by_kind = dict(
            AgentRun.objects.values_list("executor_kind").annotate(c=Count("id"))
        )
        # Emit every known kind (zero-filled) so scrapers see a stable series
        # set even before a given executor has run anything.
        run_kind_samples = [
            (kind, runs_by_kind.get(kind, 0)) for kind in AgentExecutorKind.values
        ]

        now = timezone.now()
        queued_ages = [
            (now - created).total_seconds()
            for created in AgentRun.objects.filter(
                executor_kind=AgentExecutorKind.MANAGED_RUNNER,
                status=AgentRunStatus.QUEUED,
            ).values_list("created_at", flat=True)
        ]

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
                "pi_dash_managed_runner_online",
                "Desktop-bundled (managed) runners currently online.",
                managed_online,
            ),
            _gauge(
                "pi_dash_runs_active",
                "AgentRuns in an in-flight status.",
                active_runs,
            ),
            _counter(
                "pi_dash_runs_total",
                "AgentRuns created, by executor kind.",
                run_kind_samples,
            ),
            _histogram(
                "pi_dash_managed_runner_queued_wait_seconds",
                "Age of managed runs currently queued waiting for a desktop.",
                QUEUED_WAIT_BUCKETS_SECONDS,
                queued_ages,
            ),
            _gauge(
                "pi_dash_approvals_pending",
                "ApprovalRequests waiting for a decision.",
                pending_approvals,
            ),
        ])
        return HttpResponse(body, content_type="text/plain; version=0.0.4")
