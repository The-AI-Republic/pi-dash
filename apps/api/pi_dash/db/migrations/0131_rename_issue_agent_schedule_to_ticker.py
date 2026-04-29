# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rename ``IssueAgentSchedule`` to ``IssueAgentTicker``.

Frees the word "scheduler" exclusively for the new project-level
:class:`Scheduler` / :class:`SchedulerBinding` concept (added in
``0131_project_scheduler_mvp``). The per-issue continuation clock is
re-cast as a "ticker" — its existing fields already use that vocabulary
(``tick_count``, ``last_tick_at``, ``fire_tick``).

Operations:
  - Rename the model (state-graph + auto-renamed FK reverse accessor).
  - Rename the underlying ``db_table`` from ``issue_agent_schedule`` to
    ``issue_agent_ticker``.
  - Rename the ``related_name`` on ``Issue`` from ``agent_schedule`` to
    ``agent_ticker``.
  - Rename the supporting index for clarity.

Pure rename: no column changes, no data migration, no row count change.
"""

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0130_merge_agent_schedule_and_github_sync"),
    ]

    operations = [
        migrations.RenameModel(
            old_name="IssueAgentSchedule",
            new_name="IssueAgentTicker",
        ),
        migrations.AlterModelTable(
            name="issueagentticker",
            table="issue_agent_ticker",
        ),
        migrations.AlterField(
            model_name="issueagentticker",
            name="issue",
            field=models.OneToOneField(
                on_delete=django.db.models.deletion.CASCADE,
                related_name="agent_ticker",
                to="db.issue",
            ),
        ),
        migrations.RemoveIndex(
            model_name="issueagentticker",
            name="iasched_enabled_next_run_idx",
        ),
        migrations.AddIndex(
            model_name="issueagentticker",
            index=models.Index(
                fields=["enabled", "next_run_at"],
                name="iaticker_enabled_next_run_idx",
            ),
        ),
    ]
