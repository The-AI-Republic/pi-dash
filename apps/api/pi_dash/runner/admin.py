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
    RunnerRegistrationToken,
)


@admin.register(Runner)
class RunnerAdmin(admin.ModelAdmin):
    list_display = (
        "name",
        "owner",
        "workspace",
        "status",
        "machine_token",
        "last_heartbeat_at",
    )
    list_filter = ("status", "workspace")
    search_fields = ("name", "owner__email")


@admin.register(RunnerRegistrationToken)
class RegistrationTokenAdmin(admin.ModelAdmin):
    list_display = ("id", "workspace", "created_by", "expires_at", "consumed_at")
    list_filter = ("workspace",)


@admin.register(MachineToken)
class MachineTokenAdmin(admin.ModelAdmin):
    list_display = (
        "title",
        "workspace",
        "created_by",
        "created_at",
        "last_seen_at",
        "revoked_at",
    )
    list_filter = ("workspace",)
    search_fields = ("title", "created_by__email")
    readonly_fields = ("secret_hash", "secret_fingerprint", "created_at")


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
