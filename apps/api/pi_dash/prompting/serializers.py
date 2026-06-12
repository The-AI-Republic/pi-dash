# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Serializers for the prompt-section REST surface."""

from __future__ import annotations

from rest_framework import serializers

from pi_dash.prompting.models import PromptSectionOverride


class PromptSectionOverrideSerializer(serializers.ModelSerializer):
    """Read serializer for a stored override row."""

    is_workspace_level = serializers.BooleanField(read_only=True)

    class Meta:
        model = PromptSectionOverride
        fields = [
            "id",
            "workspace",
            "user",
            "section_key",
            "body",
            "is_active",
            "version",
            "needs_attention",
            "is_workspace_level",
            "updated_by",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields


class ResolvedSectionSerializer(serializers.Serializer):
    """A section after override resolution, for the section-list / breakdown."""

    key = serializers.CharField()
    title = serializers.CharField()
    customizable = serializers.CharField()
    body = serializers.CharField()
    source = serializers.CharField()
    version = serializers.IntegerField()
    needs_attention = serializers.BooleanField()
