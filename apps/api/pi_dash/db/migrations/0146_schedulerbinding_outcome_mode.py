# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``SchedulerBinding.outcome_mode`` — what an install does with findings.

The field lives on the per-project binding (not the workspace ``Scheduler``)
so the same scheduler can behave differently across projects. The scheduler
layer still never creates issues or edits code itself; this steers the
dispatched agent run by appending a work-mode directive to its prompt (create
issues / apply fix / fix and open for review). Defaults to ``create_issue`` so
existing installs keep their current behavior with no backfill.
"""

from __future__ import annotations

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0145_schedulerbinding_pod"),
    ]

    operations = [
        migrations.AddField(
            model_name="schedulerbinding",
            name="outcome_mode",
            field=models.CharField(
                choices=[
                    ("create_issue", "Create issues"),
                    ("apply_fix", "Apply fix (open PR)"),
                    ("fix_and_review", "Fix & open for review"),
                ],
                default="create_issue",
                max_length=16,
            ),
        ),
    ]
