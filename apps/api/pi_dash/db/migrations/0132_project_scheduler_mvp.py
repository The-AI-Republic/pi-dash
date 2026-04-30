# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project Scheduler — MVP schema.

Creates:
  - ``Scheduler`` — workspace-scoped definitions (slug + prompt + cron-less
    template). Conditional unique on ``(workspace, slug)`` filtered to
    ``deleted_at IS NULL`` so uninstall+reinstall doesn't collide with
    soft-deleted tombstones (mirrors ``GithubRepositorySync``).
  - ``SchedulerBinding`` — per-project install. Conditional unique on
    ``(scheduler, project)`` for the same reason. Carries ``cron``,
    ``extra_context``, ``next_run_at``, and a ``last_run`` FK to
    ``AgentRun`` (the source of truth for run status).

A sibling migration in the ``runner`` app adds the ``AgentRun.scheduler_binding``
back-pointer; that migration depends on this one.

See ``.ai_design/project_scheduler/design.md`` §5.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0131_rename_issue_agent_schedule_to_ticker"),
        ("runner", "0009_index_renames_after_connection"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="Scheduler",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("slug", models.CharField(max_length=64)),
                ("name", models.CharField(max_length=255)),
                ("description", models.TextField(blank=True, default="")),
                ("prompt", models.TextField()),
                (
                    "source",
                    models.CharField(
                        choices=[("builtin", "Builtin"), ("manifest", "Manifest")],
                        default="builtin",
                        max_length=16,
                    ),
                ),
                ("is_enabled", models.BooleanField(default=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="scheduler_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="scheduler_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="schedulers",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "Scheduler",
                "verbose_name_plural": "Schedulers",
                "db_table": "schedulers",
                "ordering": ("-created_at",),
            },
        ),
        migrations.CreateModel(
            name="SchedulerBinding",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("cron", models.CharField(max_length=64)),
                ("extra_context", models.TextField(blank=True, default="")),
                ("enabled", models.BooleanField(default=True)),
                ("next_run_at", models.DateTimeField(blank=True, null=True)),
                ("last_error", models.TextField(blank=True, default="")),
                (
                    "actor",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="scheduler_bindings_authored",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="schedulerbinding_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="schedulerbinding_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="workspace_schedulerbinding",
                        to="db.workspace",
                    ),
                ),
                (
                    "project",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="project_schedulerbinding",
                        to="db.project",
                    ),
                ),
                (
                    "scheduler",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="bindings",
                        to="db.scheduler",
                    ),
                ),
                (
                    "last_run",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="+",
                        to="runner.agentrun",
                    ),
                ),
            ],
            options={
                "verbose_name": "Scheduler Binding",
                "verbose_name_plural": "Scheduler Bindings",
                "db_table": "scheduler_bindings",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="scheduler",
            constraint=models.UniqueConstraint(
                fields=("workspace", "slug"),
                condition=models.Q(deleted_at__isnull=True),
                name="scheduler_unique_workspace_slug_when_active",
            ),
        ),
        migrations.AddConstraint(
            model_name="schedulerbinding",
            constraint=models.UniqueConstraint(
                fields=("scheduler", "project"),
                condition=models.Q(deleted_at__isnull=True),
                name="scheduler_binding_unique_per_project_when_active",
            ),
        ),
        migrations.AddIndex(
            model_name="schedulerbinding",
            index=models.Index(
                fields=("enabled", "next_run_at"),
                name="sched_binding_due_idx",
            ),
        ),
    ]
