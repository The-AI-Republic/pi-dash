# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.validators import RegexValidator
from rest_framework import serializers

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    ApprovalRequest,
    Pod,
    Runner,
)


# Mirrors the runner-side charset rule in `runner/src/util/runner_name.rs`.
RUNNER_NAME_CHARSET = RegexValidator(
    regex=r"^[A-Za-z0-9_-]+$",
    message=(
        "runner_name may only contain letters, digits, underscore, and dash"
    ),
)


class PodSerializer(serializers.ModelSerializer):
    runner_count = serializers.SerializerMethodField()

    class Meta:
        model = Pod
        fields = [
            "id",
            "name",
            "description",
            "is_default",
            "workspace",
            "created_by",
            "runner_count",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "is_default",
            "workspace",
            "created_by",
            "runner_count",
            "created_at",
            "updated_at",
        ]

    def get_runner_count(self, pod: Pod) -> int:
        return pod.runners.count()


class PodMiniSerializer(serializers.ModelSerializer):
    class Meta:
        model = Pod
        fields = ["id", "name", "is_default"]
        read_only_fields = fields


class RunnerSerializer(serializers.ModelSerializer):
    pod_detail = PodMiniSerializer(source="pod", read_only=True)

    class Meta:
        model = Runner
        fields = [
            "id",
            "name",
            "status",
            "host_label",
            "os",
            "arch",
            "runner_version",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "pod",
            "pod_detail",
            "enrolled_at",
            "revoked_at",
            "revoked_reason",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "status",
            "os",
            "arch",
            "runner_version",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "pod_detail",
            "enrolled_at",
            "revoked_at",
            "revoked_reason",
            "created_at",
            "updated_at",
        ]


class RunnerEnrollRequestSerializer(serializers.Serializer):
    """``POST /api/v1/runner/runners/enroll/`` body."""

    enrollment_token = serializers.CharField(min_length=16, max_length=128)
    host_label = serializers.CharField(max_length=255)
    name = serializers.CharField(
        max_length=128,
        required=False,
        allow_blank=True,
        default="",
        validators=[RUNNER_NAME_CHARSET],
    )
    os = serializers.CharField(max_length=32, allow_blank=True, default="")
    arch = serializers.CharField(max_length=32, allow_blank=True, default="")
    version = serializers.CharField(max_length=32, allow_blank=True, default="")


class RunnerEnrollmentInviteSerializer(serializers.Serializer):
    """Web-UI response when a workspace admin mints a runner invite."""

    runner_id = serializers.UUIDField()
    name = serializers.CharField()
    workspace_slug = serializers.CharField()
    project_identifier = serializers.CharField()
    pod_id = serializers.UUIDField()
    enrollment_token = serializers.CharField()
    enrollment_expires_at = serializers.CharField()


class AgentRunSerializer(serializers.ModelSerializer):
    pod_detail = PodMiniSerializer(source="pod", read_only=True)

    class Meta:
        model = AgentRun
        fields = [
            "id",
            "status",
            "prompt",
            "thread_id",
            "runner",
            "work_item",
            "pod",
            "pod_detail",
            "created_by",
            "owner",
            "created_at",
            "assigned_at",
            "started_at",
            "ended_at",
            "done_payload",
            "error",
        ]
        read_only_fields = fields


class AgentRunEventSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentRunEvent
        fields = ["id", "seq", "kind", "payload", "created_at"]
        read_only_fields = fields


class ApprovalRequestSerializer(serializers.ModelSerializer):
    class Meta:
        model = ApprovalRequest
        fields = [
            "id",
            "agent_run",
            "kind",
            "payload",
            "reason",
            "status",
            "decision_source",
            "requested_at",
            "decided_at",
            "expires_at",
        ]
        read_only_fields = fields


class ApprovalDecisionSerializer(serializers.Serializer):
    decision = serializers.ChoiceField(choices=["accept", "decline"])
