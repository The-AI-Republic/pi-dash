# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the failure-policy disarm reasons to ``IssueAgentTicker`` (PDASHOSS01-183).

Choices only — no schema change. ``failure_needs_human`` is set when a run
fails for a reason a retry cannot fix (``FailurePolicy.NEEDS_HUMAN``);
``repeated_failure`` is the consecutive-same-reason backstop. Neither
auto-pauses the issue (``maybe_apply_deferred_pause`` gates on ``cap_hit``);
both are re-armed by Re-tick without granting budget.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0167_wait_budget"),
    ]

    operations = [
        migrations.AlterField(
            model_name="issueagentticker",
            name="disarm_reason",
            field=models.CharField(
                blank=True,
                choices=[
                    ("", "None"),
                    ("left_ticking_state", "Left Ticking State"),
                    ("cap_hit", "Cap Hit"),
                    ("pool_spent", "Pool Spent"),
                    ("terminal_signal", "Terminal Signal"),
                    ("user_disabled", "User Disabled"),
                    ("failure_needs_human", "Failure Needs Human"),
                    ("repeated_failure", "Repeated Failure"),
                ],
                default="",
                max_length=32,
            ),
        ),
    ]
