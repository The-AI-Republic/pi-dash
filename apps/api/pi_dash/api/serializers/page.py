# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Page serializers for the public ``/api/v1/`` surface.

Pages are a project-scoped wiki. These serializers are what agents see —
the ``pidash page`` CLI and the downstream MCP page tool both render them —
so they are deliberately free of request-specific state and safe to import
from anywhere.

The write serializers validate the *request* only. Persisting a body goes
through the live server (see :mod:`pi_dash.utils.live_document`), which the
view drives, so a serializer never saves a page on its own.
"""

# Third party imports
from rest_framework import serializers

# Module imports
from pi_dash.db.models import Page
from pi_dash.utils.markdown_converter import html_to_markdown

from .base import BaseSerializer


class PageLiteSerializer(BaseSerializer):
    """Page metadata only — no body.

    Used by the list endpoint: a project wiki can hold long documents, and an
    agent listing pages wants the index, not every body at once.
    """

    class Meta:
        model = Page
        fields = [
            "id",
            "name",
            "parent",
            "owned_by",
            "access",
            "is_locked",
            "archived_at",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields


class PageDetailSerializer(PageLiteSerializer):
    """One page, with its body in all three renderings.

    ``description_markdown`` is derived, not stored: it is produced on read by
    :func:`pi_dash.utils.markdown_converter.html_to_markdown` so the API, the
    CLI and the MCP connector never disagree about how a page reads.
    """

    description_markdown = serializers.SerializerMethodField()

    class Meta(PageLiteSerializer.Meta):
        fields = PageLiteSerializer.Meta.fields + [
            "description_html",
            "description_stripped",
            "description_markdown",
        ]
        read_only_fields = fields

    def get_description_markdown(self, obj) -> str:
        return html_to_markdown(obj.description_html)


class PageWriteSerializer(serializers.Serializer):
    """Request body shared by page create and update.

    The body is accepted as ``description_markdown`` (preferred — converted
    server-side by :func:`pi_dash.utils.markdown_converter.markdown_to_html`)
    or as Tiptap ``description_html``, never both.
    """

    name = serializers.CharField(required=False, allow_blank=False, trim_whitespace=True)
    description_markdown = serializers.CharField(required=False, allow_blank=True, trim_whitespace=False)
    description_html = serializers.CharField(required=False, allow_blank=True, trim_whitespace=False)
    parent = serializers.UUIDField(required=False, allow_null=True)
    access = serializers.ChoiceField(choices=Page.ACCESS_CHOICES, required=False)

    def validate(self, attrs):
        if "description_markdown" in attrs and "description_html" in attrs:
            raise serializers.ValidationError("Send the body as description_markdown or description_html, not both.")
        return attrs

    @property
    def has_body(self) -> bool:
        return "description_markdown" in self.validated_data or "description_html" in self.validated_data


class PageCreateSerializer(PageWriteSerializer):
    """``POST .../pages/`` — ``name`` is required."""

    name = serializers.CharField(allow_blank=False, trim_whitespace=True)


class PageUpdateSerializer(PageWriteSerializer):
    """``PATCH .../pages/{page_id}/`` — any subset, but at least one field."""

    def validate(self, attrs):
        attrs = super().validate(attrs)
        if not attrs:
            raise serializers.ValidationError(
                "Nothing to update: send at least one of name, description_markdown, "
                "description_html, parent, access."
            )
        return attrs
