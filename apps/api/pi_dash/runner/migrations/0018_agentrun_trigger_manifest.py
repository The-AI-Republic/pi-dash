# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AgentRun.trigger`` and ``AgentRun.prompt_manifest``.

``trigger`` records how a run was created (state transition / Run AI / comment /
tick / scheduler / direct) — human-vs-automatic is not derivable from
``created_by`` because ticks resolve a human creator. The prompt composer uses
it to decide whether per-user section overrides apply (design §9.1).
``prompt_manifest`` records the per-section provenance of the composed prompt.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0017_worktree_pooling"),
    ]

    operations = [
        migrations.AddField(
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
        migrations.AddField(
            model_name="agentrun",
            name="prompt_manifest",
            field=models.JSONField(blank=True, null=True),
        ),
    ]
