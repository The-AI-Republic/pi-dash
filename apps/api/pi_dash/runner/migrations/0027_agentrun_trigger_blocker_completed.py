# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the ``blocker_completed`` choice to ``AgentRun.trigger`` (PDASHOSS01-198).

Choices only — no schema change. A run labelled ``blocker_completed`` is the
immediate tick fired on a dependent when one of its ``blocked_by`` targets
reaches a completed / cancelled state.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0026_merge_agent_metadata_and_phase_kind"),
    ]

    operations = [
        migrations.AlterField(
            model_name="agentrun",
            name="trigger",
            field=models.CharField(
                choices=[
                    ("state_transition", "State transition"),
                    ("run_ai", "Run AI button"),
                    ("comment_and_run", "Comment & Run"),
                    ("tick", "Automatic tick"),
                    ("blocker_completed", "Blocker completed"),
                    ("scheduler", "Scheduler beat"),
                    ("direct", "Direct"),
                ],
                db_index=True,
                default="direct",
                max_length=24,
            ),
        ),
    ]
