# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .base import BaseSerializer
from pi_dash.db.models import User


class UserLiteSerializer(BaseSerializer):
    class Meta:
        model = User
        fields = ["id", "email", "first_name", "last_name"]
