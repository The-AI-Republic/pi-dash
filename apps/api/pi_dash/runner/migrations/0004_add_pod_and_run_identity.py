# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod + run-identity split — see .ai_design/issue_runner/design.md.

Introduces:

- ``Pod`` table (workspace-scoped group of runners sharing a work queue).
- ``Runner.pod`` FK (NOT NULL via nullable-then-tighten in-migration).
- ``AgentRun.pod`` FK (same pattern).
- ``AgentRun.created_by`` FK (NOT NULL via nullable-then-tighten; the
  authoritative principal for permission checks).
- ``AgentRun.owner`` relaxed to nullable + ``on_delete=SET_NULL``; semantic
  shift from "access principal" to "billable party," captured at assignment
  time from ``runner.owner``.

The two-step (add nullable → data pass → tighten) is kept even though
pi-dash has no production data yet, because dev/test databases may have
rows from prior local testing, and the extra RunPython passes are trivial
for the empty case.
"""

from __future__ import annotations

import uuid

from django.conf import settings
from django.db import migrations, models
import django.db.models.deletion


def _ensure_default_pods(apps, schema_editor):
    """For every existing workspace, ensure a default Pod row exists.

    Idempotent. Uses historical models so the logic is immune to later model
    edits.
    """
    Workspace = apps.get_model("db", "Workspace")
    Pod = apps.get_model("runner", "Pod")
    for ws in Workspace.objects.all():
        if Pod.objects.filter(workspace=ws).exists():
            continue
        Pod.objects.create(
            workspace=ws,
            name=f"{ws.name}-pod",
            description="Auto-created default pod (migration 0004).",
            is_default=True,
            created_by=getattr(ws, "owner", None),
        )


def _backfill_runner_pods(apps, schema_editor):
    """Attach every Runner to its workspace's default Pod."""
    Runner = apps.get_model("runner", "Runner")
    Pod = apps.get_model("runner", "Pod")
    for runner in Runner.objects.filter(pod__isnull=True):
        default_pod = Pod.objects.filter(
            workspace=runner.workspace, is_default=True
        ).first()
        if default_pod is None:
            # Shouldn't happen after _ensure_default_pods, but be defensive:
            # pick any pod in the workspace or skip.
            default_pod = Pod.objects.filter(workspace=runner.workspace).first()
        if default_pod is not None:
            runner.pod = default_pod
            runner.save(update_fields=["pod"])


def _backfill_run_identity(apps, schema_editor):
    """Populate AgentRun.pod and AgentRun.created_by for existing rows.

    - ``pod``: derived from the run's runner.pod if assigned, else workspace
      default.
    - ``created_by``: copy from existing ``owner`` (under the old model, owner
      was always the triggering user, so this is semantically correct).
    """
    AgentRun = apps.get_model("runner", "AgentRun")
    Pod = apps.get_model("runner", "Pod")
    for run in AgentRun.objects.all().select_related("runner").iterator(chunk_size=500):
        updates = []
        if run.pod_id is None:
            if run.runner_id is not None and run.runner.pod_id is not None:
                run.pod_id = run.runner.pod_id
            else:
                default_pod = Pod.objects.filter(
                    workspace_id=run.workspace_id, is_default=True
                ).first()
                if default_pod is not None:
                    run.pod = default_pod
            updates.append("pod")
        if run.created_by_id is None and run.owner_id is not None:
            run.created_by_id = run.owner_id
            updates.append("created_by")
        if updates:
            run.save(update_fields=updates)


def _assert_no_null_created_by(apps, schema_editor):
    """Fail loudly before the NOT NULL tighten rather than letting the
    AlterField raise a cryptic IntegrityError mid-migration.

    Legacy rows where both ``created_by`` and ``owner`` were NULL cannot be
    backfilled automatically; they must be resolved manually (either deleted
    or assigned to a synthetic system user) before this migration can run.
    """
    AgentRun = apps.get_model("runner", "AgentRun")
    null_ids = list(
        AgentRun.objects.filter(created_by__isnull=True).values_list("id", flat=True)[:10]
    )
    if null_ids:
        remaining = AgentRun.objects.filter(created_by__isnull=True).count()
        raise RuntimeError(
            f"Cannot tighten AgentRun.created_by to NOT NULL: {remaining} row(s) "
            f"still have created_by=NULL after backfill (sample IDs: {null_ids}). "
            f"Resolve these rows manually before retrying the migration."
        )


class Migration(migrations.Migration):

    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("runner", "0003_runner_name_unique_per_workspace"),
        ("db", "0125_branch_name_validators"),
    ]

    operations = [
        # --- 1. Create Pod table ---
        migrations.CreateModel(
            name="Pod",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("name", models.CharField(max_length=128)),
                (
                    "description",
                    models.CharField(blank=True, default="", max_length=512),
                ),
                ("is_default", models.BooleanField(default=False)),
                (
                    "deleted_at",
                    models.DateTimeField(blank=True, db_index=True, null=True),
                ),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="pods_created",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="pods",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "pod",
                "ordering": ("-is_default", "created_at"),
            },
        ),
        migrations.AddConstraint(
            model_name="pod",
            constraint=models.UniqueConstraint(
                condition=models.Q(("deleted_at__isnull", True)),
                fields=("workspace", "name"),
                name="pod_unique_name_per_workspace_when_active",
            ),
        ),
        migrations.AddConstraint(
            model_name="pod",
            constraint=models.UniqueConstraint(
                condition=models.Q(
                    ("is_default", True), ("deleted_at__isnull", True)
                ),
                fields=("workspace",),
                name="pod_one_default_per_workspace_when_active",
            ),
        ),
        migrations.AddIndex(
            model_name="pod",
            index=models.Index(
                fields=["workspace", "is_default"], name="pod_workspc_is_def_idx"
            ),
        ),
        # --- 2. Runner.pod (nullable initially to support backfill) ---
        migrations.AddField(
            model_name="runner",
            name="pod",
            field=models.ForeignKey(
                null=True,
                on_delete=django.db.models.deletion.PROTECT,
                related_name="runners",
                to="runner.pod",
            ),
        ),
        # --- 3. AgentRun fields (pod + created_by nullable; owner relaxed) ---
        migrations.AddField(
            model_name="agentrun",
            name="pod",
            field=models.ForeignKey(
                null=True,
                on_delete=django.db.models.deletion.PROTECT,
                related_name="agent_runs",
                to="runner.pod",
            ),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="created_by",
            field=models.ForeignKey(
                null=True,
                on_delete=django.db.models.deletion.PROTECT,
                related_name="agent_runs_created",
                to=settings.AUTH_USER_MODEL,
            ),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="owner",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="agent_runs",
                to=settings.AUTH_USER_MODEL,
            ),
        ),
        # --- 4. Data pass: ensure pods, attach runners and runs, fill created_by ---
        migrations.RunPython(_ensure_default_pods, reverse_code=migrations.RunPython.noop),
        migrations.RunPython(_backfill_runner_pods, reverse_code=migrations.RunPython.noop),
        migrations.RunPython(_backfill_run_identity, reverse_code=migrations.RunPython.noop),
        # Preflight: fail clearly if any AgentRun still has created_by=NULL
        # before the NOT NULL tighten below; otherwise the AlterField raises a
        # cryptic IntegrityError mid-migration.
        migrations.RunPython(
            _assert_no_null_created_by, reverse_code=migrations.RunPython.noop
        ),
        # --- 5. Tighten Runner.pod / AgentRun.pod / AgentRun.created_by to NOT NULL ---
        migrations.AlterField(
            model_name="runner",
            name="pod",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.PROTECT,
                related_name="runners",
                to="runner.pod",
            ),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="pod",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.PROTECT,
                related_name="agent_runs",
                to="runner.pod",
            ),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="created_by",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.PROTECT,
                related_name="agent_runs_created",
                to=settings.AUTH_USER_MODEL,
            ),
        ),
        # --- 6. Replace Runner name uniqueness with (pod, name) ---
        migrations.RemoveConstraint(
            model_name="runner",
            name="runner_unique_name_per_workspace",
        ),
        migrations.AddConstraint(
            model_name="runner",
            constraint=models.UniqueConstraint(
                fields=("pod", "name"),
                name="runner_unique_name_per_pod",
            ),
        ),
        migrations.AddIndex(
            model_name="runner",
            index=models.Index(fields=["pod", "status"], name="runner_pod_status_idx"),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(fields=["pod", "status"], name="agent_run_pod_status_idx"),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["created_by", "status"], name="agent_run_created_status_idx"
            ),
        ),
    ]
