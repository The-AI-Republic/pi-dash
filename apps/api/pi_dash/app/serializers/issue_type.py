# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work-item type serializers (NEWFRONT-984, ISS-129/234/235 cloud legs).

Types are workspace rows linked onto projects; the cloud type picker,
type filter and detail badge all read through the project list. The
create serializer mints the row (admins only, like labels); the view
links it onto the project.
"""

# Third Party imports
from rest_framework import serializers

# Module imports
from .base import BaseSerializer
from pi_dash.db.models import IssueType


class IssueTypeSerializer(BaseSerializer):
    class Meta:
        model = IssueType
        fields = [
            "id",
            "name",
            "description",
            "logo_props",
            "is_epic",
            "is_default",
            "is_active",
            "level",
        ]
        read_only_fields = fields


class IssueTypeCreateSerializer(BaseSerializer):
    name = serializers.CharField(max_length=255)
    description = serializers.CharField(required=False, allow_blank=True)
    logo_props = serializers.JSONField(required=False)
    is_epic = serializers.BooleanField(required=False, default=False)
    level = serializers.FloatField(required=False, default=0)

    class Meta:
        model = IssueType
        fields = ["name", "description", "logo_props", "is_epic", "level"]

    def validate_name(self, value):
        if not value.strip():
            raise serializers.ValidationError("Type name cannot be blank.")
        return value
