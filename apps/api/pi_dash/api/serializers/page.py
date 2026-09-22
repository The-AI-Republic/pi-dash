# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Read-only page serializers for the public ``/api/v1/`` surface.

Pages are a project-scoped wiki. These serializers are what agents see —
the ``pidash page`` CLI and the downstream MCP page tool both render them —
so they are deliberately free of request-specific state and safe to import
from anywhere.
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
