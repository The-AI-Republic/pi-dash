# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the cancellation barrier used by safe project-move handoffs."""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0020_runner_dev_metadata"),
    ]

    operations = [
        migrations.AlterField(
            model_name="agentrun",
            name="status",
            field=models.CharField(
                choices=[
                    ("queued", "Queued"),
                    ("assigned", "Assigned"),
                    ("waiting_for_worktree", "Waiting for Worktree"),
                    ("running", "Running"),
                    ("cancel_requested", "Cancellation Requested"),
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
