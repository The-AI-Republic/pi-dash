# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Issue.workpad`` — durable per-issue agent scratchpad.

The coding agent previously kept its cross-run state in a dedicated
``## Agent Workpad`` IssueComment. The comment thread is now reserved for
human ↔ agent conversation, so the workpad moves onto the Issue itself as
a plain markdown TextField. Existing workpad comments are left in place
and aged out — no backfill.
"""

from __future__ import annotations

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0140_scheduler_binding_rrule"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="workpad",
            field=models.TextField(blank=True, default=""),
        ),
    ]
