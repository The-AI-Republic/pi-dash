# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.validators import RegexValidator
from rest_framework import serializers

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    ApprovalKind,
    ApprovalRequest,
    ApprovalStatus,
    Pod,
    Runner,
    RunnerRegistrationToken,
)


# Mirrors the runner-side charset rule in `runner/src/util/runner_name.rs`.
# Applied on registration so the cloud rejects garbage before it hits the
# `UNIQUE(workspace_id, name)` constraint. Defense in depth.
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
        # `runners` related_name; we count active (non-revoked) runners for
        # the UI's "N runners" badge.
        return pod.runners.exclude(status="revoked").count()


class PodMiniSerializer(serializers.ModelSerializer):
    """Compact nested representation of a pod for embedding in other rows."""

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
            "os",
            "arch",
            "runner_version",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "pod",
            "pod_detail",
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
            "created_at",
            "updated_at",
        ]


class RegistrationTokenSerializer(serializers.ModelSerializer):
    class Meta:
        model = RunnerRegistrationToken
        fields = ["id", "label", "expires_at", "consumed_at", "created_at"]
        read_only_fields = fields


class RegistrationRequestSerializer(serializers.Serializer):
    # Minted tokens are ``apd_reg_`` (8) + ~32 chars of entropy. Reject
    # obvious garbage before we spend a DB round-trip on it.
    token = serializers.CharField(min_length=16, max_length=128)
    runner_name = serializers.CharField(
        min_length=1,
        max_length=128,
        validators=[RUNNER_NAME_CHARSET],
    )
    os = serializers.CharField(max_length=32)
    arch = serializers.CharField(max_length=32)
    version = serializers.CharField(max_length=32)
    protocol_version = serializers.IntegerField(min_value=1, max_value=999)


class RegistrationResponseSerializer(serializers.Serializer):
    runner_id = serializers.UUIDField()
    runner_secret = serializers.CharField()
    # Workspace the runner is permanently bound to. The runner persists this
    # in ``config.toml`` so the pidash CRUD CLI can scope REST requests
    # without asking the user to type ``--workspace`` every time.
    workspace_slug = serializers.CharField()
    # Public REST API token (``X-Api-Key``) issued alongside the runner
    # secret so the same install can also drive ``/api/v1/`` for work-item
    # CRUD via the pidash CLI. Independently revocable.
    api_token = serializers.CharField()
    heartbeat_interval_secs = serializers.IntegerField()
    protocol_version = serializers.IntegerField()
    # Project chosen during registration. The daemon persists this in
    # `config.toml` so Hello frames and CLI CRUD requests can stay scoped to
    # the project without re-prompting the user.
    project_identifier = serializers.CharField()
    # Pod the runner joined at registration time. Optional in the client so
    # older daemons can ignore it until they understand project-scoped pods.
    pod_id = serializers.CharField()


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
