# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AssistantThread.kind`` (chat | loop).

Additive, defaulted column. Existing rows are chat threads by definition, so no
backfill is needed. Loop (Auto Project Management) run threads carry
``kind="loop"`` and are hidden from the assistant thread list. See
``.ai_design/loop_project_management/design.md`` §6.4.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("assistant", "0001_initial"),
    ]

    operations = [
        migrations.AddField(
            model_name="assistantthread",
            name="kind",
            field=models.CharField(
                choices=[("chat", "Chat"), ("loop", "Loop")],
                default="chat",
                max_length=16,
            ),
        ),
    ]
