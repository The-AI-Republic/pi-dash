# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Request validation for the bulk issue endpoints (NEWFRONT-984).

Bulk update (ISS-112) applies one properties object across a selection;
bulk subscribe (ISS-115) subscribes the caller to every selected issue.
Both ride the OSS stack API ungated, per the NEWFRONT-672 precedent —
the cloud gate lives in the web_new edition module, not here.
"""

# Third Party imports
from rest_framework import serializers

# The bulk-editable set, exactly (parity/specs/issues.md §10): anything
# outside it is a client bug, not a silently ignored key.
BULK_EDITABLE_PROPERTIES = frozenset(
    {
        "state_id",
        "priority",
        "label_ids",
        "assignee_ids",
        "start_date",
        "target_date",
        "module_ids",
        "cycle_id",
        "estimate_point",
    }
)

BULK_PRIORITIES = ("urgent", "high", "medium", "low", "none")


class BulkOperationPropertiesSerializer(serializers.Serializer):
    state_id = serializers.UUIDField(required=False, allow_null=True)
    priority = serializers.ChoiceField(choices=BULK_PRIORITIES, required=False)
    label_ids = serializers.ListField(child=serializers.UUIDField(), required=False)
    assignee_ids = serializers.ListField(child=serializers.UUIDField(), required=False)
    start_date = serializers.DateField(required=False, allow_null=True)
    target_date = serializers.DateField(required=False, allow_null=True)
    module_ids = serializers.ListField(child=serializers.UUIDField(), required=False)
    cycle_id = serializers.UUIDField(required=False, allow_null=True)
    estimate_point = serializers.UUIDField(required=False, allow_null=True)

    def to_internal_value(self, data):
        if isinstance(data, dict):
            unknown = set(data) - set(self.fields)
            if unknown:
                raise serializers.ValidationError(
                    {key: "This property cannot be bulk-edited." for key in sorted(unknown)}
                )
        return super().to_internal_value(data)

    def validate(self, attrs):
        if not attrs:
            raise serializers.ValidationError("Properties must name at least one bulk-editable field.")
        start = attrs.get("start_date")
        target = attrs.get("target_date")
        if start is not None and target is not None and start > target:
            raise serializers.ValidationError("Start date cannot exceed target date")
        return attrs


class BulkOperationSerializer(serializers.Serializer):
    issue_ids = serializers.ListField(child=serializers.UUIDField(), min_length=1)
    properties = BulkOperationPropertiesSerializer(required=True)


class BulkSubscribeSerializer(serializers.Serializer):
    issue_ids = serializers.ListField(child=serializers.UUIDField(), min_length=1)
