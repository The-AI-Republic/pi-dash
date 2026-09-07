# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the issue-complexity and profile-settings migration branches.

The branches are independent: one adds ``Issue.complexity_score`` while the
other adds Cloud Agent execution fields and ``Profile.settings``. No operation
or ordering constraint is required; this migration restores a single leaf.
"""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0159_issue_complexity_score"),
        ("db", "0161_profile_settings_bag"),
    ]

    operations = []
