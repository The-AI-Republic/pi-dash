# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ]

    operations = [
        migrations.AddField(
            model_name="pod",
            name="pin_wait_budget_secs",
            field=models.PositiveIntegerField(blank=True, null=True),
        ),
    ]
