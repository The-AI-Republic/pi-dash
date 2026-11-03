# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the activity-watchdog health columns to ``RunnerLiveState``.

Runners with the passive-kill watchdog report ``agent_health`` (``healthy`` /
``unhealthy``) and ``agent_last_activity_at`` on every busy poll. Both are
nullable: NULL is a runner that predates the watchdog, which the cloud's
``reconcile_stalled_runs`` backstop keeps covering.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ]

    operations = [
        migrations.AddField(
            model_name="runnerlivestate",
            name="agent_health",
            field=models.CharField(
                blank=True,
                choices=[("healthy", "Healthy"), ("unhealthy", "Unhealthy")],
                max_length=16,
                null=True,
            ),
        ),
        migrations.AddField(
            model_name="runnerlivestate",
            name="agent_last_activity_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
    ]
