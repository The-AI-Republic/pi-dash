# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop the ``blocker_completed`` choice from ``AgentRun.trigger`` (PDASHOSS01-204).

Choices only — no schema change. 0027 added the value for the wake-on-close
listener, which fired an immediate tick on an issue's dependents when it
closed. That listener is gone: the platform no longer reads ``blocked_by`` to
decide when a wait ends, so no run is ever created with this trigger again.

Rows already labelled ``blocker_completed`` keep their value — a ``choices``
change is not validated against stored data — and read back as an unknown
trigger, which every consumer already tolerates (it is neither a human nor an
automatic issue trigger). 0027 is not rewritten; it is merged and deployed.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0028_agent_run_usage_json"),
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
                    ("scheduler", "Scheduler beat"),
                    ("direct", "Direct"),
                ],
                db_index=True,
                default="direct",
                max_length=24,
            ),
        ),
    ]
