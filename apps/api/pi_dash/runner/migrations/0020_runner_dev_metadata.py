# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0019_machine_session"),
    ]

    operations = [
        migrations.AddField(
            model_name="runner",
            name="dev_metadata",
            field=models.JSONField(blank=True, default=dict),
        ),
    ]
