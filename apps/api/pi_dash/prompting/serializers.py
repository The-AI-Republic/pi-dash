# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Serializers for the prompt-template REST surface."""

from __future__ import annotations

from rest_framework import serializers

from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.renderer import PromptSyntaxError, validate_syntax

#: Upper bound on template body size. The shipped default is ~9 KB; 100 KB is
#: ~10× headroom without allowing obvious DoS via multi-MB payloads. Enforced
#: at save time only — reading existing rows that somehow exceed this is not
#: blocked.
MAX_BODY_LENGTH = 100_000


class PromptTemplateSerializer(serializers.ModelSerializer):
    is_global_default = serializers.BooleanField(read_only=True)

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

    def validate_body(self, value: str) -> str:
        # Size cap. Rendering a huge template is cheap in Jinja but storing
        # it and re-serializing it on every list response isn't — cheap DoS
        # surface worth closing at the API boundary.
        if len(value) > MAX_BODY_LENGTH:
            raise serializers.ValidationError(
                f"template body exceeds {MAX_BODY_LENGTH}-character limit "
                f"(got {len(value)} characters)"
            )
        # Syntax-only check. We deliberately do NOT try rendering the template
        # against a fake context here — missing context variables are expected
        # at save time and don't indicate a broken template.
        try:
            validate_syntax(value)
        except PromptSyntaxError as exc:
            raise serializers.ValidationError(f"template body is invalid: {exc}")
        return value
