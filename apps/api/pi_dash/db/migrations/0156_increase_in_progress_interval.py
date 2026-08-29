# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Increase the default In Progress ticking cadence from 3 h to 12 h.

Per PDASHOSS01-78 the In Progress phase re-invokes the agent too often at
the 3 h interval; the cadence is lengthened to 12 h (43200 s) so periodic
runs are spaced further apart. Only the In Progress phase changes — the
In Review cadence (``agent_review_default_interval_seconds``) is untouched.

Only the project-level default changes; per-issue
``IssueAgentTicker.interval_seconds`` overrides are untouched. Following
the same rationale as ``0155_reduce_review_max_ticks`` a data migration is
intentionally *not* run: projects created before this change already
materialized ``10800`` into their row, and silently rewriting an
operator's stored value is more surprising than leaving it. New projects
created after this migration get ``43200``.
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
            name="agent_default_interval_seconds",
            field=models.IntegerField(default=43200),
        ),
    ]
