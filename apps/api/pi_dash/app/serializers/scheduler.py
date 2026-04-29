# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Serializers for the project-scheduler API surface.

See ``.ai_design/project_scheduler/design.md`` §7.
"""

from __future__ import annotations

from croniter import CroniterBadCronError, croniter
from rest_framework import serializers

from pi_dash.app.serializers.base import BaseSerializer
from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding


def _validate_cron_expression(value: str) -> str:
    value = (value or "").strip()
    if not value:
        raise serializers.ValidationError("cron expression is required")
    try:
        croniter(value)
    except (CroniterBadCronError, ValueError) as e:
        raise serializers.ValidationError(f"invalid cron expression: {e}")
    return value


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


class SchedulerBindingSerializer(BaseSerializer):
    scheduler_slug = serializers.CharField(source="scheduler.slug", read_only=True)
    scheduler_name = serializers.CharField(source="scheduler.name", read_only=True)
    last_run_status = serializers.SerializerMethodField()
    last_run_ended_at = serializers.SerializerMethodField()

    class Meta:
        model = SchedulerBinding
        fields = [
            "id",
            "scheduler",
            "scheduler_slug",
            "scheduler_name",
            "project",
            "workspace",
            "cron",
            "extra_context",
            "enabled",
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

    def validate_cron(self, value: str) -> str:
        return _validate_cron_expression(value)
