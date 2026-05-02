# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PR B / M1 — In Review state group, In Review state seed, and the
phase-aware cadence split on Project + IssueAgentTicker.

Operations:
  - ``AlterField`` on ``State.group`` to refresh the choices set with
    ``REVIEW``.
  - ``AddField`` × 2 on ``Project`` for review-phase cadence defaults
    (``agent_review_default_interval_seconds`` = 10800,
    ``agent_review_default_max_ticks`` = 8).
  - ``AddField`` × 3 on ``IssueAgentTicker``:
    ``review_interval_seconds`` (null override),
    ``review_max_ticks`` (null override),
    ``resume_parent_run`` (FK to AgentRun).
  - ``RunPython`` data migration: for every existing project lacking
    an In Review state, create one in the ``review`` group.

See ``.ai_design/create_review_state/design.md`` §6 and §8.
"""

from __future__ import annotations

import django.db.models.deletion
from django.db import migrations, models


REVIEW_NAME = "In Review"
REVIEW_GROUP = "review"
REVIEW_COLOR = "#5B5BD6"
REVIEW_SEQUENCE = 40000
#: Tag applied to ``State.external_source`` on rows this migration
#: inserts. Lets the reverse migration scope its delete to those rows
#: only — never touch a manually-created In Review state.
MIGRATION_TAG = "0135_review_state_and_cadence_split"


def add_in_review_state_to_projects(apps, schema_editor):
    State = apps.get_model("db", "State")
    Project = apps.get_model("db", "Project")
    for project in Project._default_manager.all().iterator():
        exists = State._default_manager.filter(
            project=project,
            name=REVIEW_NAME,
            deleted_at__isnull=True,
        ).exists()
        if exists:
            continue
        State._default_manager.create(
            name=REVIEW_NAME,
            color=REVIEW_COLOR,
            sequence=REVIEW_SEQUENCE,
            group=REVIEW_GROUP,
            default=False,
            project=project,
            workspace_id=project.workspace_id,
            external_source=MIGRATION_TAG,
        )


def remove_in_review_state_from_projects(apps, schema_editor):
    State = apps.get_model("db", "State")
    State._default_manager.filter(
        name=REVIEW_NAME,
        group=REVIEW_GROUP,
        external_source=MIGRATION_TAG,
    ).delete()


_GROUP_CHOICES = [
    ("backlog", "Backlog"),
    ("unstarted", "Unstarted"),
    ("started", "Started"),
    ("review", "Review"),
    ("completed", "Completed"),
    ("cancelled", "Cancelled"),
    ("triage", "Triage"),
]


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0134_issueagentticker_disarm_reason"),
        ("runner", "0010_agentrun_scheduler_binding"),
    ]

    operations = [
        # 1. Refresh State.group choices to include "review".
        migrations.AlterField(
            model_name="state",
            name="group",
            field=models.CharField(
                choices=_GROUP_CHOICES,
                default="backlog",
                max_length=20,
            ),
        ),
        # 2. Project review-phase cadence defaults.
        migrations.AddField(
            model_name="project",
            name="agent_review_default_interval_seconds",
            field=models.IntegerField(default=10800),
        ),
        migrations.AddField(
            model_name="project",
            name="agent_review_default_max_ticks",
            field=models.IntegerField(default=8),
        ),
        # 3. IssueAgentTicker per-issue review overrides + resume_parent_run.
        migrations.AddField(
            model_name="issueagentticker",
            name="review_interval_seconds",
            field=models.IntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="review_max_ticks",
            field=models.IntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="resume_parent_run",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="+",
                to="runner.agentrun",
            ),
        ),
        # 4. Backfill: ensure every project has an In Review state.
        migrations.RunPython(
            add_in_review_state_to_projects,
            reverse_code=remove_in_review_state_from_projects,
        ),
    ]
