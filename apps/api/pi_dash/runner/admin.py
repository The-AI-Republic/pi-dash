# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.contrib import admin

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    ApprovalRequest,
    MachineToken,
    Runner,
    RunnerForceRefresh,
    RunnerSession,
)


@admin.register(Runner)
class RunnerAdmin(admin.ModelAdmin):
    list_display = (
        "name",
        "owner",
        "workspace",
        "status",
        "host_label",
        "refresh_token_generation",
        "last_heartbeat_at",
        "revoked_at",
    )
    list_filter = ("status", "workspace")
    search_fields = ("name", "owner__email", "host_label")
    readonly_fields = (
        "refresh_token_hash",
        "refresh_token_fingerprint",
        "previous_refresh_token_hash",
        "enrollment_token_hash",
        "enrollment_token_fingerprint",
        "enrolled_at",
        "created_at",
    )


@admin.register(RunnerSession)
class RunnerSessionAdmin(admin.ModelAdmin):
    list_display = (
        "id",
        "runner",
        "protocol_version",
        "created_at",
        "last_seen_at",
        "revoked_at",
        "revoked_reason",
    )
    list_filter = ("revoked_reason",)


@admin.register(RunnerForceRefresh)
class RunnerForceRefreshAdmin(admin.ModelAdmin):
    list_display = ("runner", "min_rtg", "reason", "created_at")


@admin.register(MachineToken)
class MachineTokenAdmin(admin.ModelAdmin):
    list_display = (
        "id",
        "user",
        "workspace",
        "host_label",
        "is_service",
        "created_at",
        "last_used_at",
        "revoked_at",
    )
    list_filter = ("is_service",)
    search_fields = ("user__email", "host_label")
    readonly_fields = ("token_hash", "token_fingerprint", "created_at")


@admin.register(AgentRun)
class AgentRunAdmin(admin.ModelAdmin):
    list_display = ("id", "status", "runner", "owner", "started_at", "ended_at")
    list_filter = ("status",)
    search_fields = ("id", "runner__name")


@admin.register(AgentRunEvent)
class AgentRunEventAdmin(admin.ModelAdmin):
    list_display = ("id", "agent_run", "seq", "kind", "created_at")
    list_filter = ("kind",)


@admin.register(ApprovalRequest)
class ApprovalRequestAdmin(admin.ModelAdmin):
    list_display = ("id", "agent_run", "kind", "status", "requested_at", "decided_at")
    list_filter = ("status", "kind")
