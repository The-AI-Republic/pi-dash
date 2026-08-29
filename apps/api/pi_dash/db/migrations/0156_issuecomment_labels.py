# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import django.contrib.postgres.fields
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0155_reduce_review_max_ticks"),
    ]

    operations = [
        migrations.AddField(
            model_name="issuecomment",
            name="labels",
            field=django.contrib.postgres.fields.ArrayField(
                base_field=models.CharField(max_length=32),
                blank=True,
                default=list,
                size=8,
            ),
        ),
    ]
