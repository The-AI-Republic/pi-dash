# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod ↔ Project ↔ Runner refactor.

Reshapes the pod model so that pods are project-scoped, not
workspace-scoped. See
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§5 and §11 for the full design and rationale.

The migration is destructive: there is no faithful translation from
"one default pod per workspace" to "one default pod per project," so we
NULL out ``issues.assigned_pod_id`` (PROTECT FK), then refuse to
proceed if any rows exist in ``pod`` / ``runner`` / ``agent_run``.
Operator wipes those tables manually and reruns. Then we add the new
``Pod.project`` FK and constraints, and backfill one default pod per
existing Project.

Step ordering is significant — see the inline comments in each
operation.
"""

from __future__ import annotations

from django.db import migrations, models
import django.db.models.deletion
from django.db.models import Q


def _null_issue_assigned_pods(apps, schema_editor):
    """Detach issues from pods so the wipe can succeed.

    ``Issue.assigned_pod`` is ``on_delete=PROTECT`` (intentional — a
    pod with active issues should not be physically deletable). That
    guard fights us during the wipe-and-rebuild migration: any
    pre-existing ``Issue`` row blocks ``DELETE FROM pod`` until we
    NULL the FK first.

    After this migration, ``Issue.save()``'s auto-resolution flips to
    ``Pod.default_for_project_id(...)``, so issues that are touched
    after this runs will re-acquire a pod naturally. The dispatch
    fallback (``issue.assigned_pod or Pod.default_for_project_id(...)``)
    handles the in-flight case.
    """
    Issue = apps.get_model("db", "Issue")
    Issue.objects.filter(assigned_pod__isnull=False).update(assigned_pod=None)


def _refuse_if_legacy_rows_exist(apps, schema_editor):
    """Hard-fail if any pre-existing pod / runner / agent_run rows are
    present. Legacy rows have no faithful translation into the new
    project-scoped model.

    The error message includes the exact SQL the operator should run
    to clear the tables (FK-respecting order) before re-applying.
    """
    Pod = apps.get_model("runner", "Pod")
    Runner = apps.get_model("runner", "Runner")
    AgentRun = apps.get_model("runner", "AgentRun")

    leftover = []
    if AgentRun.objects.exists():
        leftover.append(("agent_run", AgentRun.objects.count()))
    if Runner.objects.exists():
        leftover.append(("runner", Runner.objects.count()))
    if Pod.objects.exists():
        leftover.append(("pod", Pod.objects.count()))

    if not leftover:
        return

    detail = ", ".join(f"{tbl}={n}" for tbl, n in leftover)
    raise RuntimeError(
        "Migration 0007_pod_project_relationship cannot proceed: legacy "
        f"workspace-pod data is still present ({detail}). The pod model "
        "is being reshaped from workspace-scoped to project-scoped and "
        "there is no faithful translation. Wipe the affected tables and "
        "re-apply:\n"
        "    DELETE FROM agent_run_event;\n"
        "    DELETE FROM agent_run_approval;\n"
        "    DELETE FROM agent_run;\n"
        "    DELETE FROM runner;\n"
        "    DELETE FROM pod;\n"
        "(in that order). All runners must re-register with `pidash "
        "configure --project <slug>` after the migration completes."
    )


def _backfill_default_pod_per_project(apps, schema_editor):
    """Create one default pod per existing Project.

    The new ``post_save(Project)`` signal only fires on Project create.
    Existing Project rows would otherwise be left potless, breaking
    runner registration and dispatch the moment the migration
    finishes. Idempotent: skips projects that already have a pod (in
    case the operator hand-created some between migration steps).
    """
    Project = apps.get_model("db", "Project")
    Pod = apps.get_model("runner", "Pod")

    for project in Project.objects.all().iterator(chunk_size=500):
        if Pod.objects.filter(project=project).exists():
            continue
        Pod.objects.create(
            workspace_id=project.workspace_id,
            project=project,
            name=f"{project.identifier}_pod_1",
            description="Auto-created default pod (migration 0007).",
            is_default=True,
            created_by=getattr(project, "project_lead", None)
            or getattr(project, "default_assignee", None),
        )


def _noop_reverse(apps, schema_editor):
    """The wipe + reshape is one-way; reverse is a no-op so dev can
    still ``migrate runner zero`` to drop the schema entirely.
    """
    pass


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0001_initial"),
        ("runner", "0006_machine_token"),
    ]

    operations = [
        # 1. Drop the old workspace-scoped uniqueness constraints. They
        # mention fields ("is_default per workspace", "name per
        # workspace") that the new model invalidates.
        migrations.RemoveConstraint(
            model_name="pod",
            name="pod_unique_name_per_workspace_when_active",
        ),
        migrations.RemoveConstraint(
            model_name="pod",
            name="pod_one_default_per_workspace_when_active",
        ),
        # 2. Add the new project FK as nullable so existing rows can
        # coexist briefly. The next steps wipe / backfill, then we
        # tighten to NOT NULL in step 6.
        migrations.AddField(
            model_name="pod",
            name="project",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.CASCADE,
                related_name="pods",
                to="db.project",
            ),
        ),
        # 3. NULL out issue→pod FKs. Must precede the pod wipe in step
        # 4 because the FK is PROTECT.
        migrations.RunPython(_null_issue_assigned_pods, _noop_reverse),
        # 4. Refuse to proceed if any legacy pod / runner / agent_run
        # rows survive. Operator wipes manually then reruns.
        migrations.RunPython(_refuse_if_legacy_rows_exist, _noop_reverse),
        # 5. Backfill a default pod for every existing Project. Empty
        # DB → no-op. Pre-populated DB → one pod per project.
        migrations.RunPython(_backfill_default_pod_per_project, _noop_reverse),
        # 6. Tighten Pod.project to NOT NULL. Safe now that backfill
        # has completed.
        migrations.AlterField(
            model_name="pod",
            name="project",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.CASCADE,
                related_name="pods",
                to="db.project",
            ),
        ),
        # 7. Add the new project-scoped uniqueness constraints.
        migrations.AddConstraint(
            model_name="pod",
            constraint=models.UniqueConstraint(
                fields=["project"],
                condition=Q(is_default=True) & Q(deleted_at__isnull=True),
                name="pod_one_default_per_project_when_active",
            ),
        ),
        migrations.AddConstraint(
            model_name="pod",
            constraint=models.UniqueConstraint(
                fields=["project", "name"],
                condition=Q(deleted_at__isnull=True),
                name="pod_unique_name_per_project_when_active",
            ),
        ),
    ]
