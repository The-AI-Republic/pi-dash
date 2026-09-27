# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AgentRun.failure_reason`` (PDASHOSS01-183).

Indexed CharField holding the canonical failure taxonomy value from
``pi_dash.runner.failure.RunFailureReason``. Blank for every non-failed
run; classified at write time in ``finalize_agent_run`` /
``finalize_run_terminal``. Existing failed rows are backfilled by 0031.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="failure_reason",
            field=models.CharField(blank=True, db_index=True, default="", max_length=64),
        ),
    ]
