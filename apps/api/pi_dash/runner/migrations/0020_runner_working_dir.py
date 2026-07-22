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
            name="working_dir",
            field=models.CharField(blank=True, default="", max_length=1024),
        ),
    ]
