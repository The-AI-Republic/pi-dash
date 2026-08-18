# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Give the **In Test** phase its own cadence field pair.

In Test is a sibling of In Review, not a variant of it: the two phases
tick independently and one phase's budget must never be readable or
writable through the other's fields. The interim v1 aliasing (In Test
resolving through ``agent_review_default_*`` / ``review_*``) leaked a
per-issue cap grant across the phase boundary — a "re-tick" on an
exhausted In Test issue silently inflated the next In Review budget,
and vice versa.

Adds:

- ``Project.agent_test_default_interval_seconds`` = 43200 (12 h)
- ``Project.agent_test_default_max_ticks`` = 3 (a 36 h test window)
- ``IssueAgentTicker.test_interval_seconds`` / ``test_max_ticks``
  (nullable per-issue overrides; ``null`` = inherit the project default)

No data migration: the new columns take their defaults, and nothing was
previously stored under a test-specific name to carry over. Issues
sitting In Test at deploy time move from the review cadence (8 h × 4) to
the test cadence (12 h × 3) on their next tick — an intended correction,
not a regression.
"""

from __future__ import annotations

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0157_merge_intervals_and_comment_labels"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="agent_test_default_interval_seconds",
            field=models.IntegerField(default=43200),
        ),
        migrations.AddField(
            model_name="project",
            name="agent_test_default_max_ticks",
            field=models.IntegerField(default=3),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="test_interval_seconds",
            field=models.IntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="test_max_ticks",
            field=models.IntegerField(blank=True, null=True),
        ),
    ]
