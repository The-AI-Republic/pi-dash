# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-248: the clock stops only on an explicit stop-ticking signal
# from the run. New ``stop_signal`` disarm reason; ``terminal_signal`` is
# kept for legacy rows. Choices-only change — no database DDL.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0167_wait_budget"),
    ]

    operations = [
        migrations.AlterField(
            model_name="issueagentticker",
            name="disarm_reason",
            field=models.CharField(
                blank=True,
                choices=[
                    ("", "None"),
                    ("left_ticking_state", "Left Ticking State"),
                    ("cap_hit", "Cap Hit"),
                    ("pool_spent", "Pool Spent"),
                    ("terminal_signal", "Terminal Signal"),
                    ("stop_signal", "Stop Signal"),
                    ("user_disabled", "User Disabled"),
                ],
                default="",
                max_length=32,
            ),
        ),
    ]
