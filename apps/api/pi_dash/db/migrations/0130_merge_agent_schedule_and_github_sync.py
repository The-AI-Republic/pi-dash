# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0128_relax_github_repository_sync_unique_together"),
        ("db", "0129_backfill_agent_schedules"),
    ]

    operations = []
