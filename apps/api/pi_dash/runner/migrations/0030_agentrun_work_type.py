# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stamp the effective work type on each run (PDASHOSS01-234).

Audit metadata mirroring ``phase_kind``: which work-type guidance the run's
prompt was composed with. Blank on historical rows (they predate the axis).
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="work_type",
            field=models.CharField(blank=True, default="", max_length=32),
        ),
    ]
