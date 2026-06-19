# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin endpoints for managing loop jobs and observing targets.

All behind :class:`InstanceAdminPermission`. The admin surface exposes the full
job (prompt, min_role, RRULE) — unlike the user surface (§9.1). See design §9.2.
"""

from __future__ import annotations

import re
from datetime import timedelta

from django.db.models import Count, Q
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant.models import TurnStatus
from pi_dash.bgtasks._rrule import RRuleValidationError, validate_rrule_string
from pi_dash.db.models import LoopJob, LoopTarget, SkipReason
from pi_dash.license.api.permissions import InstanceAdminPermission

_SLUG_RE = re.compile(r"^[a-z0-9-]{1,64}$")
_VALID_ROLES = {5, 15, 20}
_WRITABLE = {
    "slug",
    "name",
    "public_name",
    "public_description",
    "prompt",
    "min_role",
    "enabled",
    "dtstart",
    "rrule",
    "tzid",
}


def _job_payload(job: LoopJob) -> dict:
    return {
        "id": str(job.id),
        "slug": job.slug,
        "name": job.name,
        "public_name": job.public_name,
        "public_description": job.public_description,
        "prompt": job.prompt,
        "min_role": job.min_role,
        "enabled": job.enabled,
        "is_builtin": job.is_builtin,
        "dtstart": job.dtstart.isoformat() if job.dtstart else None,
        "rrule": job.rrule,
        "tzid": job.tzid,
        "created_at": job.created_at.isoformat() if job.created_at else None,
        "updated_at": job.updated_at.isoformat() if job.updated_at else None,
    }


def _hourly_floor_ok(rrule: str) -> bool:
    """Reject sub-hourly cadences — loop jobs fire an LLM run per membership
    edge, so the minimum sensible cadence is hourly (design §6.1)."""
    freq = ""
    for part in (rrule or "").split(";"):
        if part.startswith("FREQ="):
            freq = part[len("FREQ="):].upper()
    return freq not in ("SECONDLY", "MINUTELY")


def _validate_writes(data: dict, *, partial: bool) -> tuple[dict, Response | None]:
    """Validate and coerce a job write payload."""
    cleaned: dict = {}
    for key, value in data.items():
        if key not in _WRITABLE:
            continue  # ignore read-only / unknown keys (is_builtin, id, ...)
        cleaned[key] = value

    if "slug" in cleaned and not _SLUG_RE.match(str(cleaned["slug"])):
        return {}, Response({"error": "invalid_slug"}, status=status.HTTP_400_BAD_REQUEST)
    if "min_role" in cleaned and int(cleaned["min_role"]) not in _VALID_ROLES:
        return {}, Response({"error": "invalid_min_role"}, status=status.HTTP_400_BAD_REQUEST)
    if "rrule" in cleaned:
        rrule = str(cleaned["rrule"])
        if not rrule:
            return {}, Response(
                {"error": "invalid_rrule", "detail": "rrule is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            validate_rrule_string(rrule)
        except RRuleValidationError as exc:
            return {}, Response({"error": "invalid_rrule", "detail": str(exc)}, status=status.HTTP_400_BAD_REQUEST)
        if not _hourly_floor_ok(rrule):
            return {}, Response({"error": "rrule_too_frequent"}, status=status.HTTP_400_BAD_REQUEST)

    if not partial:
        required = {"slug", "name", "public_name", "prompt", "rrule"}
        missing = required - set(cleaned.keys())
        if missing:
            return {}, Response(
                {"error": "missing_fields", "detail": sorted(missing)}, status=status.HTTP_400_BAD_REQUEST
            )
    return cleaned, None


class LoopJobListCreateEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    def get(self, request):
        jobs = LoopJob.objects.filter(deleted_at__isnull=True).order_by("-created_at")
        return Response([_job_payload(j) for j in jobs])

    def post(self, request):
        cleaned, err = _validate_writes(request.data if isinstance(request.data, dict) else {}, partial=False)
        if err is not None:
            return err
        if LoopJob.objects.filter(slug=cleaned["slug"], deleted_at__isnull=True).exists():
            return Response({"error": "slug_taken"}, status=status.HTTP_409_CONFLICT)
        if "dtstart" not in cleaned:
            cleaned["dtstart"] = timezone.now()
        job = LoopJob.objects.create(is_builtin=False, **cleaned)
        return Response(_job_payload(job), status=status.HTTP_201_CREATED)


class LoopJobDetailEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    def get(self, request, pk):
        job = LoopJob.objects.filter(pk=pk, deleted_at__isnull=True).first()
        if job is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        payload = _job_payload(job)
        # 24h run rollup from the job's targets' last runs.
        since = timezone.now() - timedelta(hours=24)
        agg = LoopTarget.objects.filter(job=job, deleted_at__isnull=True).aggregate(
            target_count=Count("id"),
            completed=Count(
                "last_run",
                filter=Q(last_run__status=TurnStatus.COMPLETED, last_run__completed_at__gte=since),
            ),
            failed=Count(
                "last_run",
                filter=Q(last_run__status=TurnStatus.FAILED, last_run__completed_at__gte=since),
            ),
            skipped=Count("id", filter=Q(last_skipped_at__gte=since)),
        )
        payload["stats"] = agg
        return Response(payload)

    def patch(self, request, pk):
        job = LoopJob.objects.filter(pk=pk, deleted_at__isnull=True).first()
        if job is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        cleaned, err = _validate_writes(request.data if isinstance(request.data, dict) else {}, partial=True)
        if err is not None:
            return err
        if "slug" in cleaned and cleaned["slug"] != job.slug and (
            LoopJob.objects.filter(slug=cleaned["slug"], deleted_at__isnull=True).exclude(pk=job.pk).exists()
        ):
            return Response({"error": "slug_taken"}, status=status.HTTP_409_CONFLICT)
        for key, value in cleaned.items():
            setattr(job, key, value)
        job.save()
        return Response(_job_payload(job))

    def delete(self, request, pk):
        job = LoopJob.objects.filter(pk=pk, deleted_at__isnull=True).first()
        if job is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        job.delete()  # soft delete; cascades to targets via SoftDeleteModel
        return Response(status=status.HTTP_204_NO_CONTENT)


class LoopJobTargetsEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    def get(self, request, pk):
        job = LoopJob.objects.filter(pk=pk, deleted_at__isnull=True).first()
        if job is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        qs = (
            LoopTarget.objects.filter(job=job, deleted_at__isnull=True)
            .select_related("workspace", "user", "last_run")
            .order_by("-updated_at")
        )
        skip_reason = request.query_params.get("skip_reason")
        if skip_reason in SkipReason.values:
            qs = qs.filter(last_skip_reason=skip_reason)
        workspace = request.query_params.get("workspace")
        if workspace:
            qs = qs.filter(workspace__slug=workspace)
        run_status = request.query_params.get("status")
        if run_status in TurnStatus.values:
            qs = qs.filter(last_run__status=run_status)

        try:
            page = max(1, int(request.query_params.get("page", 1)))
        except (TypeError, ValueError):
            page = 1
        per = 50
        start = (page - 1) * per
        rows = list(qs[start : start + per])
        return Response(
            {
                "page": page,
                "results": [self._row(t) for t in rows],
            }
        )

    @staticmethod
    def _row(t: LoopTarget) -> dict:
        run = t.last_run
        usage = (run.usage or {}) if run else {}
        return {
            "id": str(t.id),
            "workspace_slug": t.workspace.slug if t.workspace_id else None,
            "user_email": t.user.email if t.user_id else None,
            "next_run_at": t.next_run_at.isoformat() if t.next_run_at else None,
            "last_skipped_at": t.last_skipped_at.isoformat() if t.last_skipped_at else None,
            "last_skip_reason": t.last_skip_reason,
            "last_run": (
                {
                    "status": run.status,
                    "error_code": run.error_code,
                    "model_used": run.model_used,
                    "total_tokens": usage.get("total_tokens"),
                    "completed_at": run.completed_at.isoformat() if run.completed_at else None,
                }
                if run
                else None
            ),
        }
