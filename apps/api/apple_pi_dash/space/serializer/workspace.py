# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Module imports
from .base import BaseSerializer
from apple_pi_dash.db.models import Workspace


class WorkspaceLiteSerializer(BaseSerializer):
    class Meta:
        model = Workspace
        fields = ["name", "slug", "id"]
        read_only_fields = fields
