# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .base import BaseSerializer
from apple_pi_dash.db.models import FileAsset


class FileAssetSerializer(BaseSerializer):
    class Meta:
        model = FileAsset
        fields = "__all__"
        read_only_fields = ["created_by", "updated_by", "created_at", "updated_at"]
