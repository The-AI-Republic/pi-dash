# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .base import BaseSerializer
from apple_pi_dash.db.models import AnalyticView
from apple_pi_dash.utils.issue_filters import issue_filters


class AnalyticViewSerializer(BaseSerializer):
    class Meta:
        model = AnalyticView
        fields = "__all__"
        read_only_fields = ["workspace", "query"]

    def create(self, validated_data):
        query_params = validated_data.get("query_dict", {})
        if bool(query_params):
            validated_data["query"] = issue_filters(query_params, "POST")
        else:
            validated_data["query"] = {}
        return AnalyticView.objects.create(**validated_data)

    def update(self, instance, validated_data):
        query_params = validated_data.get("query_data", {})
        if bool(query_params):
            validated_data["query"] = issue_filters(query_params, "POST")
        else:
            validated_data["query"] = {}
        validated_data["query"] = issue_filters(query_params, "PATCH")
        return super().update(instance, validated_data)
