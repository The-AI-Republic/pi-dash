# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Project.settings`` — a namespaced per-project settings bag.

Additive and empty by default: Django never reads or writes it beyond
storing it; the new Rust system (NEWPIDASH-278) is the only
reader/writer. The constant default is metadata-only on Postgres 11+.
Existing rows get ``{}`` from the column default; no data migration.
Mirrors ``Profile.settings`` (0161).

The ``RunSQL`` pins a persistent database-level default: Django's
``AddField`` only uses ``default=dict`` transiently during the column
add (``sqlmigrate`` shows the trailing ``DROP DEFAULT``), so without
it an INSERT that does not name ``settings`` — legacy Rust writers
until the entity names it (NEWPIDASH-278) — would violate the NOT
NULL constraint. Same pattern as 0172/0174.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0181_work_type_writer_defaults"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="settings",
            field=models.JSONField(default=dict),
        ),
        migrations.RunSQL(
            sql=[
                "ALTER TABLE projects ALTER COLUMN settings SET DEFAULT '{}'::jsonb;",
            ],
            reverse_sql=[
                "ALTER TABLE projects ALTER COLUMN settings DROP DEFAULT;",
            ],
        ),
    ]
