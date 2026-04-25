# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models
import django.db.models.deletion


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0004_add_pod_and_run_identity"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="pinned_runner",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="pinned_agent_runs",
                to="runner.runner",
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
                    ("paused_awaiting_input", "Paused — Awaiting Input"),
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
    ]
