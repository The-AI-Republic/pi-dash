# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from rest_framework import serializers

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    ApprovalKind,
    ApprovalRequest,
    ApprovalStatus,
    Runner,
    RunnerRegistrationToken,
)


class RunnerSerializer(serializers.ModelSerializer):
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
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields


class RegistrationTokenSerializer(serializers.ModelSerializer):
    class Meta:
        model = RunnerRegistrationToken
        fields = ["id", "label", "expires_at", "consumed_at", "created_at"]
        read_only_fields = fields


class RegistrationRequestSerializer(serializers.Serializer):
    # Minted tokens are ``apd_reg_`` (8) + ~32 chars of entropy. Reject
    # obvious garbage before we spend a DB round-trip on it.
    token = serializers.CharField(min_length=16, max_length=128)
    runner_name = serializers.CharField(min_length=1, max_length=128)
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
    heartbeat_interval_secs = serializers.IntegerField()
    protocol_version = serializers.IntegerField()


class AgentRunSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentRun
        fields = [
            "id",
            "status",
            "prompt",
            "thread_id",
            "runner",
            "work_item",
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
