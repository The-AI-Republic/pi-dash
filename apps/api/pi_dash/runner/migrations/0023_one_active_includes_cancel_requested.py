# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Include ``cancel_requested`` in the one-active-run-per-work-item
constraint. Application code (``AgentRun.is_active``, ``_active_run_for``)
already treats CANCEL_REQUESTED as occupying the active slot; without the
status in the DB condition, the direct-run POST — which relies on the
constraint alone for mutual exclusion — could create a second concurrent
run while the first is winding down."""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0022_merge_cloud_execution_and_cancel_requested"),
    ]

    operations = [
        migrations.RemoveConstraint(
            model_name="agentrun",
            name="agent_run_one_active_per_work_item",
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.UniqueConstraint(
                condition=models.Q(
                    ("work_item__isnull", False),
                    (
                        "status__in",
                        [
                            "queued",
                            "assigned",
                            "waiting_for_worktree",
                            "running",
                            "cancel_requested",
                            "awaiting_approval",
                            "awaiting_reauth",
                        ],
                    ),
                ),
                fields=("work_item",),
                name="agent_run_one_active_per_work_item",
            ),
        ),
    ]
