# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Profile.settings`` — a namespaced per-user preferences bag.

Additive and empty by default: nothing reads or writes it until a build
declares a namespace in ``pi_dash.ee.settings.user_settings``, and open source
declares none. Exists so a deployment-specific preference needs no column in
the shared schema.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0160_issue_agent_executor"),
    ]

    operations = [
        migrations.AddField(
            model_name="profile",
            name="settings",
            field=models.JSONField(default=dict),
        ),
    ]
