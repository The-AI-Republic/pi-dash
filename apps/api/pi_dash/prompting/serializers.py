# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Serializers for the prompt-template REST surface."""

from __future__ import annotations

from rest_framework import serializers

from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.renderer import PromptRenderError, render


class PromptTemplateSerializer(serializers.ModelSerializer):
    is_global_default = serializers.BooleanField(read_only=True)
    can_edit = serializers.SerializerMethodField(read_only=True)

    class Meta:
        model = PromptTemplate
        fields = [
            "id",
            "workspace",
            "name",
            "body",
            "is_active",
            "version",
            "is_global_default",
            "can_edit",
            "updated_by",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "workspace",
            "is_active",
            "version",
            "updated_by",
            "created_at",
            "updated_at",
        ]

    def get_can_edit(self, obj: PromptTemplate) -> bool:
        # Workspace-scoped rows are editable by workspace admins (the view gates
        # the mutation endpoints). The global default is never editable via this
        # surface — platform operators edit it out-of-band.
        return not obj.is_global_default

    def validate_body(self, value: str) -> str:
        # Fail loud on Jinja syntax errors at save time rather than at the next
        # run, where a broken template would take down an AgentRun with a
        # `render-failed` reason.
        try:
            render(value, {})
        except PromptRenderError as exc:
            # StrictUndefined will complain about missing context vars — that's
            # expected and fine at save time. Only real syntax errors should
            # block the save.
            msg = str(exc).lower()
            if "undefined" in msg or "strictundefined" in msg:
                return value
            raise serializers.ValidationError(f"template body is invalid: {exc}")
        return value
