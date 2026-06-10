# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Worktree pooling cloud touchpoints.

Adds the non-terminal ``waiting_for_worktree`` run status (a run assigned to a
single-tenant runner that is queueing locally for a worktree lease), a
display-only ``queue_position`` on ``AgentRun``, and a ``free_worktrees``
capacity hint on ``Runner``. All additive; no data migration. See
``.ai_design/worktree_pooling/design.md`` §6.1 / §6.4.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0016_agent_run_refused"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="queue_position",
            field=models.PositiveSmallIntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="runner",
            name="free_worktrees",
            field=models.IntegerField(blank=True, null=True),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="status",
            field=models.CharField(
                choices=[
                    ("queued", "Queued"),
                    ("assigned", "Assigned"),
                    ("waiting_for_worktree", "Waiting for Worktree"),
                    ("running", "Running"),
                    ("awaiting_approval", "Awaiting Approval"),
                    ("awaiting_reauth", "Awaiting Reauth"),
                    ("paused_awaiting_input", "Paused — Awaiting Input"),
                    ("blocked", "Blocked"),
                    ("completed", "Completed"),
                    ("failed", "Failed"),
                    ("cancelled", "Cancelled"),
                    ("refused", "Refused"),
                ],
                db_index=True,
                default="queued",
                max_length=24,
            ),
        ),
    ]
