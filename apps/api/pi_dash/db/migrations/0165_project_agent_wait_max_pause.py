# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Project.agent_wait_max_pause_seconds`` (PDASHOSS01-198).

The cap on how long an agent's ``Waiting on:`` workpad marker may pause the
issue's cadence ticks. Defaults to 7 days; existing rows take the default.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0164_unify_ticking_cadence_3h"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="agent_wait_max_pause_seconds",
            field=models.IntegerField(default=604800),
        ),
    ]
