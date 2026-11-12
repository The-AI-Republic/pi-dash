# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-machine supported agent kinds (PDASHOSS01-142).

``dev_machine`` gains ``supported_agents``: the kebab-case agent kinds the
machine's daemon binary understands, advertised on machine-session open.
The default empty list means "unknown" (an older daemon that advertises
nothing), which consumers must read as "offer every agent", never "offer
none".
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0039_issue_assigned_runner"),
    ]

    operations = [
        migrations.AddField(
            model_name="devmachine",
            name="supported_agents",
            field=models.JSONField(blank=True, default=list),
        ),
        # Django drops the column default after backfilling. Keep a
        # database-level default too, so writers that do not go through the
        # Django model (the Rust API, raw SQL) can still insert rows without
        # naming the new NOT NULL column (the 0034 / 0036 convention).
        migrations.RunSQL(
            sql=["ALTER TABLE dev_machine ALTER COLUMN supported_agents SET DEFAULT '[]'::jsonb"],
            reverse_sql=["ALTER TABLE dev_machine ALTER COLUMN supported_agents DROP DEFAULT"],
        ),
    ]
