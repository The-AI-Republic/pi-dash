# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Issue.assigned_pod`` nullable FK to ``runner.Pod``.

Issues pin to a pod; dispatch uses the pinned pod if set, else falls back to
``workspace.default_pod``. See ``.ai_design/issue_runner/design.md`` §4.4.
"""

from django.db import migrations, models
import django.db.models.deletion


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0125_branch_name_validators"),
        ("runner", "0004_add_pod_and_run_identity"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="assigned_pod",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.PROTECT,
                related_name="assigned_issues",
                to="runner.pod",
            ),
        ),
    ]
