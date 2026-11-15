# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Keep older Python/Rust writers compatible during shared-schema rollout.

Those clients omit the new required columns. ORM defaults only protect new
model versions; persistent database defaults also protect older writers.
"""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0173_issue_assigned_runner"),
        ("runner", "0039_issue_assigned_runner"),
    ]

    operations = [
        migrations.RunSQL(
            "ALTER TABLE agent_run ALTER COLUMN pin_kind SET DEFAULT 'locality'; "
            "ALTER TABLE projects ALTER COLUMN dedicated_runner_lost_action SET DEFAULT 'fail_to_backlog';",
            reverse_sql=(
                "ALTER TABLE agent_run ALTER COLUMN pin_kind DROP DEFAULT; "
                "ALTER TABLE projects ALTER COLUMN dedicated_runner_lost_action DROP DEFAULT;"
            ),
        ),
    ]
