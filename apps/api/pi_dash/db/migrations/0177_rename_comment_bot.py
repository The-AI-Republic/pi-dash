# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rename the comment bot to "Pi Dash Bot" (agent naming convention P6a).

New bot users are created with last name "Bot"
(`pi_dash/orchestration/workpad.py:32` and the Rust
`AGENT_USER_LAST_NAME`). This migration renames the existing row:
`last_name` "Agent" → "Bot" and `display_name` "Pi Dash Bot".

The `last_name="Agent"` filter leaves manual edits alone. Data-only:
no schema change. Irreversible by design (a reverse would re-create
the retired "Agent" last name); `noop_reverse` matches the backfill
precedent.
"""

from django.db import migrations


def forwards(apps, schema_editor):
    User = apps.get_model("db", "User")
    User.objects.filter(
        username="pi_dash_agent", is_bot=True, last_name="Agent"
    ).update(
        last_name="Bot",
        display_name="Pi Dash Bot",
    )


def noop_reverse(apps, schema_editor):
    pass


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0176_update_timezone_choices_pytz_2025_2"),
    ]

    operations = [
        migrations.RunPython(forwards, noop_reverse),
    ]
