# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Writes to an existing :class:`SchedulerBinding`.

One implementation shared by the web app surface
(``pi_dash/app/views/scheduler``) and the external ``/api/v1/`` surface
(``pi_dash/api/views/scheduler``), so the two cannot drift: both validate
through ``SchedulerBindingSerializer`` and recompute ``next_run_at`` the
same way.
"""

from __future__ import annotations

from typing import Mapping, Optional

from django.utils import timezone

from pi_dash.db.models import SchedulerBinding
from pi_dash.runner.models import AgentRun

#: Editing any of these changes when the binding fires next.
CADENCE_FIELDS = ("dtstart", "rrule", "rdates", "exdates", "tzid")


def update_binding(binding: SchedulerBinding, data: Mapping) -> SchedulerBinding:
    """Apply a partial edit to ``binding`` and return it.

    Raises ``rest_framework.serializers.ValidationError`` on bad input.
    """
    from pi_dash.app.serializers.scheduler import SchedulerBindingSerializer
    from pi_dash.bgtasks.scheduler import _next_fire_for_binding_checked

    serializer = SchedulerBindingSerializer(binding, data=data, partial=True)
    serializer.is_valid(raise_exception=True)
    serializer.save()
    # If any RRULE-bundle field was touched, recompute next_run_at so the
    # scanner honours the new schedule on its next tick.
    if any(k in data for k in CADENCE_FIELDS):
        binding.refresh_from_db()
        nxt, parse_error = _next_fire_for_binding_checked(binding, now=timezone.now())
        # A clean parse with no future occurrence must CLEAR next_run_at
        # (the edit made the series finite and already over) — keeping the
        # stale time would fire a rule past its end. The serializer has
        # already validated the bundle, so parse errors shouldn't happen;
        # leave next_run_at untouched if one does.
        if parse_error is None and binding.next_run_at != nxt:
            binding.next_run_at = nxt
            binding.save(update_fields=["next_run_at", "updated_at"])
    return binding


def run_now_conflict(binding: SchedulerBinding) -> Optional[tuple[str, str]]:
    """Why ``binding`` cannot be fired right now, as ``(code, message)``.

    ``None`` when it can. Each case is one the scanner's fire path would
    skip — or, for an exhausted series, answer by disabling the binding —
    so making the binding due would not produce a run.
    """
    from pi_dash.bgtasks.scheduler import (
        NON_TERMINAL_STATUSES,
        _next_fire_for_binding,
    )

    if not binding.enabled:
        return "binding_disabled", "The scheduler is disabled on this project."
    if not binding.scheduler.is_enabled:
        return "scheduler_disabled", "The scheduler definition is disabled."
    if AgentRun.objects.filter(
        scheduler_binding=binding, status__in=NON_TERMINAL_STATUSES
    ).exists():
        return "run_active", "A run from this scheduler is already active."
    if _next_fire_for_binding(binding, now=timezone.now()) is None:
        return (
            "no_future_occurrence",
            "The schedule has no future occurrence; edit the cadence first.",
        )
    return None


def mark_due_now(binding: SchedulerBinding) -> SchedulerBinding:
    """Make ``binding`` due, so the scanner fires it on its next tick.

    The fire path then advances ``next_run_at`` from the RRULE bundle as it
    does for any firing, so the regular schedule resumes by itself.
    """
    binding.next_run_at = timezone.now()
    binding.save(update_fields=["next_run_at", "updated_at"])
    return binding
