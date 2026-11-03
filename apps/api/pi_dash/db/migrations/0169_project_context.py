# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-294: agent context moves out of ``Project.description`` into a
# dedicated ``Project.context`` field. The built-in prompts now render
# ``context`` only, so existing descriptions are copied across to keep every
# project's prompt text unchanged on upgrade. ``description`` is left as is.

from django.db import migrations, models
from django.db.models import F


def backfill_context_from_description(apps, schema_editor):
    Project = apps.get_model("db", "Project")
    # _base_manager: soft-deleted projects are backfilled too, so a restored
    # project behaves like every other one.
    Project._base_manager.exclude(description="").filter(context="").update(context=F("description"))


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0168_ticker_stop_signal_disarm_reason"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="context",
            field=models.TextField(blank=True, default="", verbose_name="Project Context"),
        ),
        # Reverse is a no-op: dropping the column discards the copies.
        migrations.RunPython(backfill_context_from_description, migrations.RunPython.noop),
    ]
