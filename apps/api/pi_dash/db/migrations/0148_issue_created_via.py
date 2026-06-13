# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0147_merge_fable_audit_and_scheduler_outcome"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="created_via",
            field=models.CharField(blank=True, max_length=32, null=True),
        ),
    ]
