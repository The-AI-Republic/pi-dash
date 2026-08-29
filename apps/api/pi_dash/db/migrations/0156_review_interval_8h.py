# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Lengthen the default In Review ticking interval from 3 h to 8 h.

Per PDASHOSS01-70 the review-phase cadence is stretched so the runner
re-reviews an In Review issue every 8 h instead of every 3 h. Each review
tick is a full re-read of the thread and PR; a wider gap leaves more room
for a human to weigh in between passes without exhausting the review
budget prematurely.

Only the project-level default changes; per-issue
``IssueAgentTicker.review_interval_seconds`` overrides are untouched, and
the In Progress cadence (``agent_default_interval_seconds`` = 10800) is
unchanged. As with 0155, a data migration is intentionally *not* run:
projects created before this change already materialized ``10800`` into
their row, and silently rewriting an operator's stored value is more
surprising than leaving it. New projects created after this migration get
``28800``.
"""

from __future__ import annotations

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0155_reduce_review_max_ticks"),
    ]

    operations = [
        migrations.AlterField(
            model_name="project",
            name="agent_review_default_interval_seconds",
            field=models.IntegerField(default=28800),
        ),
    ]
