# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from urllib.parse import urlparse

from rest_framework import serializers

from pi_dash.assistant.models import AssistantThread, ProviderKind, UserLLMConfig


class AssistantThreadSerializer(serializers.ModelSerializer):
    has_active_turn = serializers.SerializerMethodField()

    class Meta:
        model = AssistantThread
        fields = ["id", "title", "is_archived", "has_active_turn", "created_at", "updated_at"]
        read_only_fields = ["id", "created_at", "updated_at", "has_active_turn"]

    def get_has_active_turn(self, obj) -> bool:
        return obj.active_turn_id is not None


# Note: message wire format is produced by ``events.message_envelope`` so the
# list endpoint and the SSE stream emit byte-identical shapes (kind->role,
# display_content->content). There is intentionally no message serializer here.


class UserLLMConfigSerializer(serializers.ModelSerializer):
    api_key = serializers.CharField(write_only=True, required=False, allow_blank=True, max_length=512)
    has_api_key = serializers.BooleanField(read_only=True)

    class Meta:
        model = UserLLMConfig
        fields = ["provider_kind", "base_url", "model_name", "api_key", "has_api_key", "last_verified_at"]
        read_only_fields = ["has_api_key", "last_verified_at"]

    def validate_model_name(self, value):
        value = (value or "").strip()
        if not value:
            raise serializers.ValidationError("A model name is required.")
        return value

    def validate_base_url(self, value):
        value = (value or "").strip().rstrip("/")
        if not value:
            return value
        parsed = urlparse(value)
        if parsed.scheme not in ("http", "https"):
            raise serializers.ValidationError("base_url must be an http(s) URL.")
        if parsed.username or parsed.password:
            raise serializers.ValidationError("base_url must not contain credentials.")
        return value

    def validate_api_key(self, value):
        if value and len(value) < 8:
            raise serializers.ValidationError("API key looks too short.")
        return value

    def validate(self, attrs):
        provider = attrs.get("provider_kind") or getattr(self.instance, "provider_kind", ProviderKind.OPENAI_COMPATIBLE)
        base_url = attrs.get("base_url", getattr(self.instance, "base_url", ""))
        if provider == ProviderKind.OPENAI_COMPATIBLE and not base_url:
            raise serializers.ValidationError(
                {"base_url": "base_url is required for OpenAI-compatible providers."}
            )
        return attrs
