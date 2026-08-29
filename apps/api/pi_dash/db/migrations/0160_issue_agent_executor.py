# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue execution target override.

NULL means "inherit ``Project.default_agent_executor``", which is where every
existing issue starts — so the project-level default is unchanged by this
migration and the field is purely additive.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0159_merge_executor_and_test_state"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="agent_executor",
            field=models.CharField(
                blank=True,
                choices=[("local_runner", "Local Runner"), ("cloud_agent", "Pi Dash Cloud Agent")],
                max_length=24,
                null=True,
            ),
        ),
    ]
