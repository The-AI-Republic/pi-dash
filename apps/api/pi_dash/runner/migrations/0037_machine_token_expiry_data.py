# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill ``MachineToken.expires_at`` (built_engine_as_runner §13).

Existing tokens get 60 days from their last use, or 60 days from the upgrade
when they were never used, so a token in use keeps working. Runs after the
dispatch data migration (0035), the last step of the section 13 order.
Reversing clears the column, which means "never expires" again.
"""

from django.db import migrations

FORWARD_SQL = """
UPDATE machine_token
   SET expires_at = COALESCE(last_used_at, now()) + interval '60 days'
 WHERE expires_at IS NULL
"""

REVERSE_SQL = "UPDATE machine_token SET expires_at = NULL"


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0036_engine_credential_machine_identity"),
    ]

    operations = [
        migrations.RunSQL(sql=FORWARD_SQL, reverse_sql=REVERSE_SQL),
    ]
