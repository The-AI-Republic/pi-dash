# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Engine-scoped device grants (``pidash login --engine``).

PIDESKAPP-122, ``.ai_design/built_engine_as_runner/design.md`` §4.3, §4.6:

- ``scope``: ``cli`` (today's grant) or ``engine`` (engine credential only).
- ``dev_machine_id``: the machine an engine grant is for, when known.
- ``interactive_approval``: approved by a person on the browser page rather
  than silently by the desktop app; only that clears an engine sign-out.
- ``api_token``: the device-flow APIToken the grant was redeemed for.

Additive; existing rows are ``cli`` grants approved non-interactively.
"""

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0171_issue_assigned_runner"),
    ]

    operations = [
        migrations.AddField(
            model_name="clidevicecode",
            name="scope",
            field=models.CharField(default="cli", max_length=16),
        ),
        migrations.AddField(
            model_name="clidevicecode",
            name="dev_machine_id",
            field=models.UUIDField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="clidevicecode",
            name="interactive_approval",
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name="clidevicecode",
            name="api_token",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="device_codes",
                to="db.apitoken",
            ),
        ),
        # Database-level defaults for writers that bypass the Django model.
        migrations.RunSQL(
            sql=[
                "ALTER TABLE cli_device_codes ALTER COLUMN scope SET DEFAULT 'cli'",
                "ALTER TABLE cli_device_codes ALTER COLUMN interactive_approval SET DEFAULT false",
            ],
            reverse_sql=[
                "ALTER TABLE cli_device_codes ALTER COLUMN scope DROP DEFAULT",
                "ALTER TABLE cli_device_codes ALTER COLUMN interactive_approval DROP DEFAULT",
            ],
        ),
    ]
