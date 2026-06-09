# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Serializers for the project-scheduler API surface.

See ``.ai_design/project_scheduler/design.md`` §7 and
``.ai_design/project_scheduler_calendar/decisions.md`` §1-2 for the
iCal-shaped recurrence model that replaced cron in migration 0140.
"""

from __future__ import annotations

import re
from datetime import datetime
from typing import Iterable
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from rest_framework import serializers

from pi_dash.app.serializers.base import BaseSerializer
from pi_dash.bgtasks._rrule import RRuleValidationError, validate_rrule_string
from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding
from pi_dash.runner.models import Pod


EXTRA_CONTEXT_MAX_LENGTH = 16 * 1024

# Cap the JSON-list extras stored on a binding — RDATE/EXDATE are
# expanded on every fire and every occurrences-endpoint request. A
# pathological 10k-entry list would slow each tick.
RDATE_EXDATE_MAX_LENGTH = 256

_HEX_COLOR_RE = re.compile(r"^#[0-9a-fA-F]{6}$")


def _validate_color(value: str) -> str:
    value = (value or "").strip().lower()
    if not _HEX_COLOR_RE.match(value):
        raise serializers.ValidationError(
            "color must be a 7-character hex string like '#3b82f6'"
        )
    return value


def _validate_iso_datetime_list(values: Iterable, field: str) -> list[str]:
    """Validate an rdates/exdates JSON list: each item must be an ISO datetime string."""
    if values is None:
        return []
    if not isinstance(values, list):
        raise serializers.ValidationError(
            {field: "must be a JSON array of ISO 8601 datetime strings"}
        )
    if len(values) > RDATE_EXDATE_MAX_LENGTH:
        raise serializers.ValidationError(
            {field: f"must contain at most {RDATE_EXDATE_MAX_LENGTH} entries"}
        )
    out: list[str] = []
    for i, raw in enumerate(values):
        if not isinstance(raw, str):
            raise serializers.ValidationError(
                {field: f"item {i} must be a string, got {type(raw).__name__}"}
            )
        try:
            parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
        except ValueError as e:
            raise serializers.ValidationError(
                {field: f"item {i} is not a valid ISO 8601 datetime: {e}"}
            )
        out.append(parsed.isoformat())
    return out


class SchedulerSerializer(BaseSerializer):
    active_binding_count = serializers.SerializerMethodField()

    class Meta:
        model = Scheduler
        fields = [
            "id",
            "workspace",
            "slug",
            "name",
            "description",
            "prompt",
            "color",
            "source",
            "is_enabled",
            "active_binding_count",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "workspace",
            "source",
            "active_binding_count",
            "created_at",
            "updated_at",
        ]

    def get_active_binding_count(self, obj: Scheduler) -> int:
        # When called over a queryset where the field is annotated, use that;
        # otherwise fall back to a count query. The list view annotates.
        annotated = getattr(obj, "_active_binding_count", None)
        if annotated is not None:
            return annotated
        return obj.bindings.filter(deleted_at__isnull=True).count()

    def validate_color(self, value: str) -> str:
        return _validate_color(value)


class SchedulerBindingSerializer(BaseSerializer):
    scheduler_slug = serializers.CharField(source="scheduler.slug", read_only=True)
    scheduler_name = serializers.CharField(source="scheduler.name", read_only=True)
    scheduler_color = serializers.CharField(source="scheduler.color", read_only=True)
    last_run_status = serializers.SerializerMethodField()
    last_run_ended_at = serializers.SerializerMethodField()
    # Optional pod override. Scoped to active pods; the cross-project check
    # lives in `validate()` (the project is known there via instance/context).
    # NULL = use the project's default pod at fire time.
    pod = serializers.PrimaryKeyRelatedField(
        queryset=Pod.objects.filter(deleted_at__isnull=True),
        required=False,
        allow_null=True,
    )
    # Joined pod name so the calendar / list can render the chosen pod without
    # a second fetch. Null when the binding uses the project default.
    pod_name = serializers.CharField(source="pod.name", read_only=True, default=None)

    class Meta:
        model = SchedulerBinding
        fields = [
            "id",
            "scheduler",
            "scheduler_slug",
            "scheduler_name",
            "scheduler_color",
            "project",
            "workspace",
            "dtstart",
            "tzid",
            "rrule",
            "rdates",
            "exdates",
            "extra_context",
            "enabled",
            "pod",
            "pod_name",
            "next_run_at",
            "last_run",
            "last_run_status",
            "last_run_ended_at",
            "last_error",
            "actor",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "workspace",
            "pod_name",
            "next_run_at",
            "last_run",
            "last_run_status",
            "last_run_ended_at",
            "last_error",
            "actor",
            "created_at",
            "updated_at",
        ]

    def get_last_run_status(self, obj: SchedulerBinding):
        return obj.last_run.status if obj.last_run_id else None

    def get_last_run_ended_at(self, obj: SchedulerBinding):
        return obj.last_run.ended_at if obj.last_run_id else None

    def validate_rrule(self, value: str) -> str:
        value = (value or "").strip()
        # Empty rrule means single-shot at dtstart — fine. validate_rrule_string
        # is a no-op for empty input but the cross-field check below catches
        # the "no rrule + no dtstart" case.
        if value:
            # Strip a leading "RRULE:" if a client sent the full iCal-line
            # prefix. dateutil handles both forms but we canonicalize to the
            # bare "FREQ=..." form for storage.
            if value.upper().startswith("RRULE:"):
                value = value[len("RRULE:"):]
            try:
                # dtstart from attrs may not be set yet at field-validation
                # time; we pass None and the validator uses a placeholder.
                validate_rrule_string(value)
            except RRuleValidationError as e:
                raise serializers.ValidationError(str(e))
        return value

    def validate_rdates(self, value):
        return _validate_iso_datetime_list(value, "rdates")

    def validate_exdates(self, value):
        return _validate_iso_datetime_list(value, "exdates")

    def validate_tzid(self, value: str) -> str:
        value = (value or "UTC").strip()
        if value == "UTC":
            return value
        try:
            ZoneInfo(value)
        except ZoneInfoNotFoundError:
            raise serializers.ValidationError(
                f"tzid {value!r} is not a recognized IANA timezone"
            )
        return value

    def validate_extra_context(self, value: str) -> str:
        if value and len(value) > EXTRA_CONTEXT_MAX_LENGTH:
            raise serializers.ValidationError(
                f"extra_context must be at most {EXTRA_CONTEXT_MAX_LENGTH} characters"
            )
        return value

    def validate(self, attrs):
        # On update, lock `scheduler` and `project` — the design says swap
        # via uninstall + reinstall, not in-place repointing (would leave
        # workspace/project inconsistent with scheduler.workspace).
        if self.instance is not None:
            for locked in ("scheduler", "project"):
                if locked in attrs and attrs[locked] != getattr(self.instance, locked):
                    raise serializers.ValidationError(
                        {locked: f"{locked} cannot be changed; uninstall and re-install"}
                    )
        # Cross-field RRULE validation now that we have both dtstart and rrule.
        # Prefer attrs (incoming changes) over the instance (existing values).
        rrule_str = attrs.get(
            "rrule",
            getattr(self.instance, "rrule", "") if self.instance else "",
        )
        dtstart = attrs.get(
            "dtstart",
            getattr(self.instance, "dtstart", None) if self.instance else None,
        )
        if rrule_str:
            try:
                validate_rrule_string(rrule_str, dtstart=dtstart)
            except RRuleValidationError as e:
                raise serializers.ValidationError({"rrule": str(e)})
        # A chosen pod must belong to the binding's project. Resolve the
        # project from the incoming data (create sends `project`), the existing
        # instance (update), or the view-supplied context (belt-and-suspenders
        # for the create path, where the view also injects project at save()).
        pod = attrs.get("pod")
        if pod is not None:
            incoming_project = attrs.get("project")
            project_id = (
                getattr(incoming_project, "id", None)
                or (self.instance.project_id if self.instance is not None else None)
                or getattr(self.context.get("project"), "id", None)
            )
            if project_id is not None and pod.project_id != project_id:
                raise serializers.ValidationError(
                    {"pod": "pod must belong to the same project as this scheduler install"}
                )
        return super().validate(attrs)
