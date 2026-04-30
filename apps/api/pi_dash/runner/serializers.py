# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.validators import RegexValidator
from rest_framework import serializers

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    ApprovalRequest,
    Connection,
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


class ConnectionSerializer(serializers.ModelSerializer):
    """Web-API representation of a Connection.

    The status field is derived from ``enrolled_at`` — pending until the
    daemon enrolls, active afterwards. Secret material is never included.
    """

    status = serializers.CharField(read_only=True)
    runner_count = serializers.SerializerMethodField()

    class Meta:
        model = Connection
        fields = [
            "id",
            "name",
            "host_label",
            "status",
            "workspace",
            "created_by",
            "secret_fingerprint",
            "enrolled_at",
            "last_seen_at",
            "created_at",
            "revoked_at",
            "runner_count",
        ]
        read_only_fields = [
            "id",
            "status",
            "workspace",
            "created_by",
            "secret_fingerprint",
            "enrolled_at",
            "last_seen_at",
            "created_at",
            "revoked_at",
            "runner_count",
        ]

    def get_runner_count(self, conn: Connection) -> int:
        return conn.runners.count()


class RunnerSerializer(serializers.ModelSerializer):
    pod_detail = PodMiniSerializer(source="pod", read_only=True)

    class Meta:
        model = Runner
        fields = [
            "id",
            "name",
            "status",
            "os",
            "arch",
            "runner_version",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "pod",
            "pod_detail",
            "connection",
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
            "connection",
            "created_at",
            "updated_at",
        ]


class EnrollmentRequestSerializer(serializers.Serializer):
    """``POST /api/v1/runner/connections/enroll/`` body."""

    token = serializers.CharField(min_length=16, max_length=128)
    host_label = serializers.CharField(max_length=255, allow_blank=True, default="")
    os = serializers.CharField(max_length=32, allow_blank=True, default="")
    arch = serializers.CharField(max_length=32, allow_blank=True, default="")
    version = serializers.CharField(max_length=32, allow_blank=True, default="")


class EnrollmentResponseSerializer(serializers.Serializer):
    connection_id = serializers.UUIDField()
    connection_secret = serializers.CharField()
    workspace_slug = serializers.CharField()
    heartbeat_interval_secs = serializers.IntegerField()
    protocol_version = serializers.IntegerField()


class RunnerCreateRequestSerializer(serializers.Serializer):
    """``POST /api/v1/runner/connections/<id>/runners/`` body.

    The runner UUID is minted by the daemon (shared util between CLI + TUI)
    and presented here so cloud and local config agree from the start.
    """

    runner_id = serializers.UUIDField()
    name = serializers.CharField(
        min_length=1, max_length=128, validators=[RUNNER_NAME_CHARSET]
    )
    project = serializers.CharField(max_length=128)
    pod = serializers.CharField(max_length=128, allow_blank=True, default="")
    os = serializers.CharField(max_length=32, allow_blank=True, default="")
    arch = serializers.CharField(max_length=32, allow_blank=True, default="")
    version = serializers.CharField(max_length=32, allow_blank=True, default="")
    protocol_version = serializers.IntegerField(min_value=1, max_value=999, default=3)


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
