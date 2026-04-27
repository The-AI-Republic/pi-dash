# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill the new ``Paused`` state into every existing project.

The Paused state belongs to the ``backlog`` group and is the destination for
issues whose periodic-tick budget is exhausted (and for users who want to
park work without abandoning it). New projects pick the entry up from the
seed file (``seeds/data/states.json``); this migration handles the existing
ones.

See ``.ai_design/issue_ticking_system/design.md`` §4.1 and §12.1.
"""

from django.db import migrations


PAUSED_NAME = "Paused"
PAUSED_GROUP = "backlog"
PAUSED_COLOR = "#DC2626"
PAUSED_SEQUENCE = 17500


def add_paused_state_to_projects(apps, schema_editor):
    """Create one ``Paused`` state per project that doesn't already have one.

    Uses ``state_unique_name_project_when_deleted_at_null``: skip projects
    whose Paused state already exists (idempotent under re-runs and against
    workspaces seeded after this migration ships).
    """
    State = apps.get_model("db", "State")
    Project = apps.get_model("db", "Project")

    for project in Project._default_manager.all().iterator():
        exists = State.objects.filter(
            project=project,
            name=PAUSED_NAME,
            deleted_at__isnull=True,
        ).exists()
        if exists:
            continue
        State._default_manager.create(
            name=PAUSED_NAME,
            color=PAUSED_COLOR,
            sequence=PAUSED_SEQUENCE,
            group=PAUSED_GROUP,
            default=False,
            project=project,
            workspace_id=project.workspace_id,
        )


def remove_paused_state_from_projects(apps, schema_editor):
    """Hard-delete the rows we created so the migration is reversible.

    A future re-run of M2 would recreate the state cleanly.
    """
    State = apps.get_model("db", "State")
    State._default_manager.filter(name=PAUSED_NAME, group=PAUSED_GROUP).delete()


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0127_issue_agent_schedule"),
    ]

    operations = [
        migrations.RunPython(
            add_paused_state_to_projects,
            reverse_code=remove_paused_state_from_projects,
        ),
    ]
