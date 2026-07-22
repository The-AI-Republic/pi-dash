# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the ``test`` state group and its default ``In Test`` state.

Operations:
  - ``AlterField`` on ``State.group`` to refresh the choices set with
    ``TEST``.
  - ``RunPython`` data migration: for every existing project lacking an
    In Test state, create one in the ``test`` group.

Unlike ``0135_review_state_and_cadence_split``, the ``test`` group is
**not** wired to any ticking cadence — it is a manual/agent-set marker
that the AI agent can move an issue to when a task needs testing. No
``PhaseConfig`` entry is added, so issues in ``In Test`` do not
auto-tick.
"""

from __future__ import annotations

from django.db import migrations, models


TEST_NAME = "In Test"
TEST_GROUP = "test"
TEST_COLOR = "#14B8A6"
TEST_SEQUENCE = 42500
#: Tag applied to ``State.external_source`` on rows this migration
#: inserts. Lets the reverse migration scope its delete to those rows
#: only — never touch a manually-created In Test state.
MIGRATION_TAG = "0154_test_state_group"


def add_in_test_state_to_projects(apps, schema_editor):
    State = apps.get_model("db", "State")
    Project = apps.get_model("db", "Project")
    for project in Project._default_manager.all().iterator():
        exists = State._default_manager.filter(
            project=project,
            name=TEST_NAME,
            group=TEST_GROUP,
            deleted_at__isnull=True,
        ).exists()
        if exists:
            continue
        State._default_manager.create(
            name=TEST_NAME,
            color=TEST_COLOR,
            sequence=TEST_SEQUENCE,
            group=TEST_GROUP,
            default=False,
            project=project,
            workspace_id=project.workspace_id,
            external_source=MIGRATION_TAG,
        )


def remove_in_test_state_from_projects(apps, schema_editor):
    State = apps.get_model("db", "State")
    State._default_manager.filter(
        name=TEST_NAME,
        group=TEST_GROUP,
        external_source=MIGRATION_TAG,
    ).delete()


_GROUP_CHOICES = [
    ("backlog", "Backlog"),
    ("unstarted", "Unstarted"),
    ("started", "Started"),
    ("review", "Review"),
    ("test", "Test"),
    ("completed", "Completed"),
    ("cancelled", "Cancelled"),
    ("triage", "Triage"),
]


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0153_workspace_join_request"),
    ]

    operations = [
        # 1. Refresh State.group choices to include "test".
        migrations.AlterField(
            model_name="state",
            name="group",
            field=models.CharField(
                choices=_GROUP_CHOICES,
                default="backlog",
                max_length=20,
            ),
        ),
        # 2. Backfill: ensure every project has an In Test state.
        migrations.RunPython(
            add_in_test_state_to_projects,
            reverse_code=remove_in_test_state_from_projects,
        ),
    ]
