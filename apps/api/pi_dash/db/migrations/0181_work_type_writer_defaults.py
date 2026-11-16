# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Keep older writers compatible with the work-type and failure-reason columns.

``db/0179``, ``runner/0040`` and ``runner/0042`` added NOT NULL columns and
then dropped their ``''`` database default, as Django does. A server that
predates those migrations (an older Django or Rust release still running
while a newer one migrates the shared database) omits the columns on
insert, so creating a project or an agent run fails until it is replaced.

Persistent ``''`` database defaults fix that, as ``db/0174`` did for
``pin_kind`` and ``dedicated_runner_lost_action``. ``''`` is what the ORM
writes for these fields anyway: a blank ``default_work_type`` resolves to
``general`` (``prompting/work_types.py``), and blank ``failure_reason`` and
``work_type`` mean "not recorded". The model fields are unchanged, so this
is database-only and leaves no model-state drift.
"""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0180_rename_comment_bot"),
        ("runner", "0042_agentrun_work_type"),
    ]

    operations = [
        migrations.RunSQL(
            sql=[
                "ALTER TABLE projects ALTER COLUMN default_work_type SET DEFAULT ''",
                "ALTER TABLE agent_run ALTER COLUMN failure_reason SET DEFAULT ''",
                "ALTER TABLE agent_run ALTER COLUMN work_type SET DEFAULT ''",
            ],
            reverse_sql=[
                "ALTER TABLE projects ALTER COLUMN default_work_type DROP DEFAULT",
                "ALTER TABLE agent_run ALTER COLUMN failure_reason DROP DEFAULT",
                "ALTER TABLE agent_run ALTER COLUMN work_type DROP DEFAULT",
            ],
        ),
    ]
