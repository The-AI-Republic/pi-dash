# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unify the ticking cadence to 3 h (10800 s) across every stage.

Per PDASHOSS01-167 the three ticking stages (In Progress, In Review, In
Test) should re-invoke the agent on the same rhythm — **3 h (10800 s)**
between ticks. The three per-stage interval columns on ``Project`` stay in
place so cadences can diverge again in the future, but their default values
are all set to 10800.

Unlike ``0156_increase_in_progress_interval`` / ``0156_review_interval_8h``
(which deliberately skipped a data migration), this migration **does** run a
data migration and rewrites *every* existing ``Project`` row to 10800 for
all three columns — the goal is a single unified cadence, so live rows sitting
at the old mixed defaults (10800 / 10800 / 43200 on older projects,
43200 / 28800 / 43200 on newer ones) are all brought in line, not only the
rows still on a particular old default.

Budget (``agent_default_max_ticks`` / ``agent_retick_grant``), jitter, the
scanner frequency, and the cap-hit / auto-pause behaviour are unchanged.
"""

from __future__ import annotations

from django.db import migrations, models

UNIFIED_INTERVAL_SECONDS = 10800  # 3 h

INTERVAL_COLUMNS = (
    "agent_default_interval_seconds",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
)


def unify_project_intervals(apps, schema_editor):
    """Bring every existing project row to the unified 3 h cadence in all
    three interval columns."""
    Project = apps.get_model("db", "Project")
    Project.objects.all().update(
        agent_default_interval_seconds=UNIFIED_INTERVAL_SECONDS,
        agent_review_default_interval_seconds=UNIFIED_INTERVAL_SECONDS,
        agent_test_default_interval_seconds=UNIFIED_INTERVAL_SECONDS,
    )


def noop(apps, schema_editor):
    """Reverse is a no-op: the unified value is indistinguishable per row
    from a legitimately configured 3 h cadence, so there is nothing safe to
    restore."""


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0163_ticker_one_clock_one_pool"),
    ]

    operations = [
        migrations.AlterField(
            model_name="project",
            name="agent_default_interval_seconds",
            field=models.IntegerField(default=10800),
        ),
        migrations.AlterField(
            model_name="project",
            name="agent_review_default_interval_seconds",
            field=models.IntegerField(default=10800),
        ),
        migrations.AlterField(
            model_name="project",
            name="agent_test_default_interval_seconds",
            field=models.IntegerField(default=10800),
        ),
        migrations.RunPython(unify_project_intervals, noop),
    ]
