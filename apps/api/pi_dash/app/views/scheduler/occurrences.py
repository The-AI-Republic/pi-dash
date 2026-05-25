# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Occurrences endpoint for the project scheduler calendar.

GET /workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/?from=&to=

Returns a flat array of occurrence records in the requested window, merging:

- **Past** ``AgentRun`` rows whose ``started_at`` falls in the window and
  whose ``scheduler_binding`` is on this project — keyed ``"past"``.
- **Future** RRULE-expanded occurrences from currently-enabled bindings —
  keyed ``"scheduled"``.

See .ai_design/project_scheduler_calendar/decisions.md §3.

Caps:

- Date window must be <= 90 days. Reject 400 with a hint on overshoot.
- Response is capped at ``OCCURRENCE_CAP`` rows; truncation sets
  ``has_more=true`` and a ``next_window_start`` pointer.

Caching (PR2 leaves this simple; future work can introduce Valkey):

- The endpoint does not currently cache responses. The expansion cost of
  90 days * N bindings * common cadences is bounded by the response cap.
  A future PR can add a per-(slug, project_id, from, to, max_run_id) key
  in Valkey with a 30s/5min split TTL — left as a TODO for now.
"""

from __future__ import annotations

import logging
from datetime import datetime, timedelta, timezone as dt_timezone
from typing import Iterable, Optional

from django.shortcuts import get_object_or_404
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.views.base import BaseAPIView
from pi_dash.bgtasks._rrule import occurrences_between
from pi_dash.db.models import Project, SchedulerBinding
from pi_dash.runner.models import AgentRun

logger = logging.getLogger(__name__)


MAX_WINDOW_DAYS = 90
OCCURRENCE_CAP = 5000


def _parse_iso(value: str) -> Optional[datetime]:
    """Parse an ISO 8601 datetime, accepting both Z and ±HH:MM tz suffixes.

    Returns ``None`` if the value can't be parsed.
    """
    if not value:
        return None
    try:
        d = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if d.tzinfo is None:
        d = d.replace(tzinfo=dt_timezone.utc)
    return d.astimezone(dt_timezone.utc)


def _as_datetimes(raw: Iterable) -> list[datetime]:
    """Coerce a JSONField list of ISO strings into tz-aware UTC datetimes."""
    out: list[datetime] = []
    for item in raw or ():
        if isinstance(item, datetime):
            d = item
        elif isinstance(item, str):
            d = _parse_iso(item)
            if d is None:
                continue
        else:
            continue
        if d.tzinfo is None:
            d = d.replace(tzinfo=dt_timezone.utc)
        out.append(d)
    return out


class ProjectSchedulerOccurrencesEndpoint(BaseAPIView):
    """GET /workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/?from=&to=

    Returns ``{occurrences, has_more, next_window_start}`` for the project's
    calendar tab. Permission gate matches the bindings list endpoint
    (read for any project role).
    """

    @allow_permission(
        allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
        level="PROJECT",
    )
    def get(self, request, slug, project_id):
        # Ensure the project exists and lives in the right workspace.
        # ``get_object_or_404`` short-circuits with 404 if either fails.
        get_object_or_404(Project, pk=project_id, workspace__slug=slug)

        now = timezone.now()
        window_start = _parse_iso(request.query_params.get("from", ""))
        window_end = _parse_iso(request.query_params.get("to", ""))

        # Defaults: previous 30 days through next 30 days. Symmetric so the
        # client can hit / without query params and see "this month-ish."
        if window_start is None:
            window_start = now - timedelta(days=30)
        if window_end is None:
            window_end = now + timedelta(days=30)

        if window_end < window_start:
            return Response(
                {"error": "invalid_window", "detail": "`to` must be >= `from`"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if (window_end - window_start) > timedelta(days=MAX_WINDOW_DAYS):
            return Response(
                {
                    "error": "window_too_large",
                    "detail": f"date window must be <= {MAX_WINDOW_DAYS} days",
                },
                status=status.HTTP_400_BAD_REQUEST,
            )

        # ---------- Future occurrences (RRULE expansion) ----------
        # Pull *enabled* bindings on this project whose parent scheduler is
        # enabled and not soft-deleted. Mirrors the scanner's filter
        # (see bgtasks/scheduler.py:scan_due_bindings).
        future_bindings = list(
            SchedulerBinding.objects.filter(
                workspace__slug=slug,
                project_id=project_id,
                enabled=True,
                scheduler__is_enabled=True,
                scheduler__deleted_at__isnull=True,
                project__deleted_at__isnull=True,
            )
            .select_related("scheduler")
        )

        occurrences: list[dict] = []
        truncated_at: Optional[datetime] = None

        # Future expansion is bounded by the cap; we expand earliest first
        # so the response is dense at the start of the window if we hit it.
        # The cap applies to the merged (past + future) total, not just
        # future — but past comes from a query (fast) and future from
        # expansion (potentially expensive), so we apply the cap during
        # expansion to keep wall-clock time bounded.
        remaining_cap = OCCURRENCE_CAP
        future_start = max(window_start, now)
        for binding in future_bindings:
            if remaining_cap <= 0:
                break
            expanded, hit_cap = occurrences_between(
                dtstart=binding.dtstart,
                rrule_str=binding.rrule or "",
                tzid=binding.tzid or "UTC",
                rdates=_as_datetimes(binding.rdates),
                exdates=_as_datetimes(binding.exdates),
                window_start=future_start,
                window_end=window_end,
                cap=remaining_cap,
            )
            for occ in expanded:
                occurrences.append({
                    "binding_id": str(binding.id),
                    "scheduler_id": str(binding.scheduler_id),
                    "scheduler_name": binding.scheduler.name,
                    "scheduler_color": binding.scheduler.color or "#3b82f6",
                    "dtstart": occ.isoformat(),
                    "tzid": binding.tzid or "UTC",
                    "kind": "scheduled",
                    "agent_run_id": None,
                    "status": None,
                })
            remaining_cap = OCCURRENCE_CAP - len(occurrences)
            if hit_cap and remaining_cap <= 0:
                truncated_at = expanded[-1] if expanded else None
                break

        # ---------- Past occurrences (AgentRun rows) ----------
        # Past slice covers [window_start, min(window_end, now)] — no point
        # joining future AgentRuns; the runner hasn't created them yet.
        past_end = min(window_end, now)
        if window_start < past_end:
            past_runs = (
                AgentRun.objects.filter(
                    workspace__slug=slug,
                    scheduler_binding__project_id=project_id,
                    scheduler_binding__isnull=False,
                    started_at__gte=window_start,
                    started_at__lte=past_end,
                )
                .select_related("scheduler_binding__scheduler")
                .order_by("started_at")
            )
            for run in past_runs:
                if len(occurrences) >= OCCURRENCE_CAP:
                    break
                sched = run.scheduler_binding.scheduler
                occurrences.append({
                    "binding_id": str(run.scheduler_binding_id),
                    "scheduler_id": str(sched.id),
                    "scheduler_name": sched.name,
                    "scheduler_color": sched.color or "#3b82f6",
                    "dtstart": run.started_at.isoformat(),
                    "tzid": run.scheduler_binding.tzid or "UTC",
                    "kind": "past",
                    "agent_run_id": str(run.id),
                    "status": run.status,
                })

        # Sort by dtstart so the client can render time-axis week view
        # without re-sorting. Past + future are interleaved naturally.
        occurrences.sort(key=lambda o: o["dtstart"])

        has_more = truncated_at is not None
        next_window_start = truncated_at.isoformat() if truncated_at else None

        return Response(
            {
                "occurrences": occurrences,
                "has_more": has_more,
                "next_window_start": next_window_start,
            },
            status=status.HTTP_200_OK,
        )
