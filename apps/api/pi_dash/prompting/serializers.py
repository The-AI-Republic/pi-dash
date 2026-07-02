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
    # Pristine registry default, for an override-vs-default diff in the editor.
    default_body = serializers.CharField()
    source = serializers.CharField()
    version = serializers.IntegerField()
    needs_attention = serializers.BooleanField()
    # Section-level capability: whether an admin may set a workspace override and
    # whether a member may keep a personal one. The client combines these with
    # the caller's role to decide which editors to surface.
    editable_at_workspace = serializers.BooleanField()
    editable_at_personal = serializers.BooleanField()
