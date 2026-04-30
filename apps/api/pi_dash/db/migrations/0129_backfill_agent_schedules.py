# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill ``IssueAgentSchedule`` rows for existing In Progress issues.

After this migration runs, the scanner will start ticking these issues on
their first ``next_run_at``. We deliberately set ``next_run_at = NOW() +
project_default_interval + jitter`` (rather than ``NOW()``) so a deploy
doesn't stampede every existing in-progress issue at once.

See ``.ai_design/issue_ticking_system/design.md`` §12.1 (M3).
"""

import random
from datetime import timedelta

from django.db import migrations
from django.utils import timezone


DELEGATION_STATE_NAME = "In Progress"
JITTER_FRACTION = 0.1


def backfill_schedules(apps, schema_editor):
    Issue = apps.get_model("db", "Issue")
    IssueAgentSchedule = apps.get_model("db", "IssueAgentSchedule")

    now = timezone.now()
    issues = Issue._default_manager.filter(
        state__group="started",
        state__name=DELEGATION_STATE_NAME,
        deleted_at__isnull=True,
    ).select_related("project")

    for issue in issues.iterator():
        if IssueAgentSchedule._default_manager.filter(issue=issue).exists():
            continue
        project = issue.project
        interval = getattr(project, "agent_default_interval_seconds", 10800) or 10800
        offset = random.uniform(0, interval * JITTER_FRACTION)
        next_run_at = now + timedelta(seconds=interval + offset)
        IssueAgentSchedule._default_manager.create(
            issue=issue,
            interval_seconds=None,
            max_ticks=None,
            user_disabled=False,
            next_run_at=next_run_at,
            tick_count=0,
            enabled=getattr(project, "agent_ticking_enabled", True),
        )


def remove_backfilled_schedules(apps, schema_editor):
    """Reverse: hard-delete every IssueAgentSchedule.

    The migration is reversed only when the entire feature is being rolled
    back, so dropping all rows is acceptable; M1 will drop the table on
    further reverse.
    """
    IssueAgentSchedule = apps.get_model("db", "IssueAgentSchedule")
    IssueAgentSchedule._default_manager.all().delete()


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0128_paused_state"),
    ]

    operations = [
        migrations.RunPython(
            backfill_schedules,
            reverse_code=remove_backfilled_schedules,
        ),
    ]
