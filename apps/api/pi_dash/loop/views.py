# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User-facing "Auto Project Management" settings endpoints.

A user can see the instance's enabled jobs and toggle each on/off (plus a master
pause). Absence of a preference row = enabled. These are the user's own
preferences, so there is no workspace-role gate. The route segment is
``auto-pm`` and the payload never exposes prompts or the word "loop". See
design §9.1.
"""

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.db.models import LoopJob, LoopUserPreference
from pi_dash.loop.serializers import public_job_payload


def _master_enabled(user) -> bool:
    pref = (
        LoopUserPreference.objects.filter(user=user, job__isnull=True, deleted_at__isnull=True)
        .values_list("enabled", flat=True)
        .first()
    )
    return True if pref is None else bool(pref)


def _job_enabled_map(user) -> dict:
    """slug-keyed effective enabled state for every enabled job."""
    off_job_ids = set(
        LoopUserPreference.objects.filter(
            user=user, job__isnull=False, enabled=False, deleted_at__isnull=True
        ).values_list("job_id", flat=True)
    )
    return off_job_ids


def _settings_payload(user) -> dict:
    off_job_ids = _job_enabled_map(user)
    jobs = LoopJob.objects.filter(enabled=True, deleted_at__isnull=True).order_by("public_name")
    return {
        "enabled": _master_enabled(user),
        "jobs": [public_job_payload(j, enabled=j.id not in off_job_ids) for j in jobs],
    }


def _read_enabled(request) -> tuple[bool | None, Response | None]:
    """Extract a single boolean ``enabled`` from the request body, or an error
    response. Rejects any other key so the contract is toggle-only."""
    data = request.data if isinstance(request.data, dict) else {}
    if set(data.keys()) - {"enabled"} or "enabled" not in data:
        return None, Response({"error": "invalid_payload"}, status=status.HTTP_400_BAD_REQUEST)
    value = data.get("enabled")
    if not isinstance(value, bool):
        return None, Response({"error": "invalid_payload"}, status=status.HTTP_400_BAD_REQUEST)
    return value, None


class AutoPMSettingsEndpoint(BaseAPIView):
    """GET the user's Auto PM settings; PATCH the master pause switch."""

    def get(self, request):
        return Response(_settings_payload(request.user))

    def patch(self, request):
        enabled, err = _read_enabled(request)
        if err is not None:
            return err
        LoopUserPreference.objects.update_or_create(
            user=request.user,
            job=None,
            deleted_at__isnull=True,
            defaults={"enabled": enabled},
        )
        return Response(_settings_payload(request.user))


class AutoPMJobEndpoint(BaseAPIView):
    """PATCH a single job's on/off state for the requesting user."""

    def patch(self, request, slug):
        job = LoopJob.objects.filter(slug=slug, enabled=True, deleted_at__isnull=True).first()
        if job is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        enabled, err = _read_enabled(request)
        if err is not None:
            return err
        LoopUserPreference.objects.update_or_create(
            user=request.user,
            job=job,
            deleted_at__isnull=True,
            defaults={"enabled": enabled},
        )
        return Response(_settings_payload(request.user))
