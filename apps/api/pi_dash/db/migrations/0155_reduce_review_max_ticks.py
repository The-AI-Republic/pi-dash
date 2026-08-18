# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Reduce the default In Review ticking budget from 8 to 4.

Runner agents were re-reviewing an issue up to 8 times before ticking
stopped. Per PDASHOSS01-68 the review cadence is shortened to 4 ticks
(≈12 h at the 3 h interval) after which the issue simply stays In Review
for a human — the runner never promotes it to Done.

Only the project-level default changes; per-issue
``IssueAgentTicker.review_max_ticks`` overrides are untouched. Existing
projects that never customized the default pick up the new value because
the column stores the schema default only at row-creation time — but a
data migration is intentionally *not* run here: projects created before
this change already materialized ``8`` into their row, and silently
rewriting an operator's stored value is more surprising than leaving it.
New projects created after this migration get ``4``.
"""

from __future__ import annotations

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0154_test_state_group"),
    ]

    operations = [
        migrations.AlterField(
            model_name="project",
            name="agent_review_default_max_ticks",
            field=models.IntegerField(default=4),
        ),
    ]
