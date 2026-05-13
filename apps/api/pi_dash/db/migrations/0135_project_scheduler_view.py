# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0134_issueagentticker_disarm_reason"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="scheduler_view",
            field=models.BooleanField(default=False),
        ),
    ]
