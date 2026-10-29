# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Read-only serializers for the external (`/api/v1/`) scheduler surface.

The `pidash` CLI and the Pi Dash MCP connector consume these — see
``docs/mcp-connector.md`` ("Schedulers (read-only)"). Every field is an
explicit whitelist: nothing secret lives on the models today, and keeping
the list explicit means a future credential column cannot leak by default.

Write access is intentionally absent (PDASHOSS01-225 is read-only; write
is a follow-up with its own permission model).
"""

from rest_framework import serializers

from pi_dash.api.serializers.base import BaseSerializer
from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding
from pi_dash.runner.models import AgentRun


class SchedulerBindingAPISerializer(BaseSerializer):
    """One install of a scheduler onto one project: cadence + runtime state."""

    scheduler_slug = serializers.CharField(source="scheduler.slug", read_only=True)
    scheduler_name = serializers.CharField(source="scheduler.name", read_only=True)
    pod_name = serializers.CharField(source="pod.name", read_only=True, default=None)
    last_run_status = serializers.SerializerMethodField()
    last_run_started_at = serializers.SerializerMethodField()
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
            "dtstart",
            "tzid",
            "rrule",
            "rdates",
            "exdates",
            "extra_context",
            "enabled",
            "outcome_mode",
            "pod",
            "pod_name",
            "next_run_at",
            "last_run",
            "last_run_status",
            "last_run_started_at",
            "last_run_ended_at",
            "last_error",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields

    def get_last_run_status(self, obj: SchedulerBinding):
        return obj.last_run.status if obj.last_run_id else None

    def get_last_run_started_at(self, obj: SchedulerBinding):
        return obj.last_run.started_at if obj.last_run_id else None

    def get_last_run_ended_at(self, obj: SchedulerBinding):
        return obj.last_run.ended_at if obj.last_run_id else None


class SchedulerAPISerializer(BaseSerializer):
    """A workspace scheduler definition, with the bindings the caller may see.

    ``bindings`` reads the ``visible_bindings`` attribute the view attaches
    (via ``Prefetch(..., to_attr="visible_bindings")``), so the workspace
    list never leaks installs on projects the caller is not a member of.
    """

    bindings = serializers.SerializerMethodField()

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
            "bindings",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields

    def get_bindings(self, obj: Scheduler):
        bindings = getattr(obj, "visible_bindings", [])
        return SchedulerBindingAPISerializer(bindings, many=True).data


class SchedulerRunAPISerializer(serializers.ModelSerializer):
    """One scheduler-fired agent run, for the run-history endpoint.

    ``issues`` is the audit trail of work items the run wrote to: every
    issue carrying a comment whose ``speaker_agent_run_id`` is this run.
    The view precomputes the mapping in one query and passes it through
    ``context["issues_by_run"]``.
    """

    scheduler = serializers.UUIDField(
        source="scheduler_binding.scheduler_id", read_only=True, default=None
    )
    project = serializers.UUIDField(
        source="scheduler_binding.project_id", read_only=True, default=None
    )
    issues = serializers.SerializerMethodField()

    class Meta:
        model = AgentRun
        fields = [
            "id",
            "scheduler",
            "scheduler_binding",
            "project",
            "status",
            "trigger",
            "phase_kind",
            "error_code",
            "created_at",
            "started_at",
            "ended_at",
            "issues",
        ]
        read_only_fields = fields

    def get_issues(self, obj: AgentRun):
        issues_by_run = self.context.get("issues_by_run", {})
        return issues_by_run.get(obj.id, [])
