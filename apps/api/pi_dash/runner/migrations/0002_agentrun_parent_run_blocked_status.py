# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0001_initial"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="parent_run",
            field=models.ForeignKey(
                blank=True,
                help_text=(
                    "Prior run this attempt follows up on; null for an issue's "
                    "initial run."
                ),
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="follow_up_runs",
                to="runner.agentrun",
            ),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="status",
            field=models.CharField(
                choices=[
                    ("queued", "Queued"),
                    ("assigned", "Assigned"),
                    ("running", "Running"),
                    ("awaiting_approval", "Awaiting Approval"),
                    ("awaiting_reauth", "Awaiting Reauth"),
                    ("blocked", "Blocked"),
                    ("completed", "Completed"),
                    ("failed", "Failed"),
                    ("cancelled", "Cancelled"),
                ],
                db_index=True,
                default="queued",
                max_length=24,
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["work_item", "status"],
                name="agent_run_work_it_f1b9d8_idx",
            ),
        ),
    ]
