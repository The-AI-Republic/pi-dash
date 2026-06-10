# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``SchedulerBinding.pod`` — optional per-binding pod override.

NULL keeps the existing behavior (the dispatcher resolves the project's
default pod at fire time). When set, runs fired by the binding target the
chosen pod instead. SET_NULL so a hard pod delete degrades to the project
default rather than orphaning the binding.
"""

from __future__ import annotations

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0144_issuecomment_speaker_metadata"),
        ("runner", "0015_dev_machine_visibility"),
    ]

    operations = [
        migrations.AddField(
            model_name="schedulerbinding",
            name="pod",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="scheduler_bindings",
                to="runner.pod",
            ),
        ),
    ]
