# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AgentRun.scheduler_binding`` back-pointer.

Counterpart to ``db.0132_project_scheduler_mvp``. Depends on it so the
target table (``scheduler_bindings``) exists before the FK is added.

Project-scoped runs (scheduler ticks) carry this back-pointer instead of
``work_item``. Exactly one of the two is set per run; the dispatcher
enforces the invariant.

See ``.ai_design/project_scheduler/design.md`` §5.
"""

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0009_index_renames_after_connection"),
        ("db", "0132_project_scheduler_mvp"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="scheduler_binding",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="agent_runs",
                to="db.schedulerbinding",
            ),
        ),
    ]
