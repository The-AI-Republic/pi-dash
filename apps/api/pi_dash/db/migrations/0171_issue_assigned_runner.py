# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``Issue.assigned_runner``: dispatch an issue to one specific runner.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §7. Nullable
and ``SET_NULL``, so existing rows stay pod-level; the runner data migration
``runner.0035`` backfills it for issues that were set to the built-in engine.
"""

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0170_issueview_anchor"),
        ("runner", "0034_engine_dispatch_fields"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="assigned_runner",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="assigned_issues",
                to="runner.runner",
            ),
        ),
    ]
