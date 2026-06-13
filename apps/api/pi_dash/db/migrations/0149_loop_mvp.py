# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop (Auto Project Management) — MVP schema + builtin seed.

Creates:
  - ``LoopJob`` — instance-level (prompt + timer) catalog entries. Conditional
    unique on ``slug`` filtered to ``deleted_at IS NULL``.
  - ``LoopTarget`` — per-membership-edge cursor (job × workspace × user) with a
    nullable FK to the hidden ``assistant.AssistantThread`` and to the last
    ``assistant.AssistantTurn`` (the run *is* the turn — no LoopRun model).
  - ``LoopUserPreference`` — a user's per-job opt-out (or master pause when
    ``job`` is NULL). Absence of a row = enabled.

Seeds the one MVP builtin job (``auto-close-merged``) ``enabled=False`` — the
operator flips it on from apps/admin after a smoke test (design §13).

See ``.ai_design/loop_project_management/design.md`` §6, §8.1.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


def seed_builtin_jobs(apps, schema_editor):
    from pi_dash.loop.builtins import BUILTIN_LOOP_JOBS

    LoopJob = apps.get_model("db", "LoopJob")
    now = __import__("django.utils.timezone", fromlist=["now"]).now()
    for spec in BUILTIN_LOOP_JOBS:
        LoopJob.objects.update_or_create(
            slug=spec.slug,
            deleted_at__isnull=True,
            defaults={
                "name": spec.name,
                "public_name": spec.public_name,
                "public_description": spec.public_description,
                "prompt": spec.prompt,
                "min_role": spec.min_role,
                "enabled": False,  # operator enables after smoke test (design §13)
                "is_builtin": True,
                "dtstart": now,
                "rrule": spec.rrule,
                "tzid": spec.tzid,
            },
        )


def unseed_builtin_jobs(apps, schema_editor):
    from pi_dash.loop.builtins import BUILTIN_LOOP_JOBS

    LoopJob = apps.get_model("db", "LoopJob")
    LoopJob.objects.filter(slug__in=[s.slug for s in BUILTIN_LOOP_JOBS]).delete()


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0148_issue_created_via"),
        ("assistant", "0002_assistantthread_kind"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="LoopJob",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("slug", models.CharField(max_length=64)),
                ("name", models.CharField(max_length=255)),
                ("public_name", models.CharField(max_length=255)),
                ("public_description", models.TextField(blank=True, default="")),
                ("prompt", models.TextField()),
                ("min_role", models.PositiveSmallIntegerField(default=15)),
                ("enabled", models.BooleanField(default=True)),
                ("is_builtin", models.BooleanField(default=True)),
                ("dtstart", models.DateTimeField()),
                ("rrule", models.CharField(max_length=255)),
                ("tzid", models.CharField(default="UTC", max_length=64)),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="loopjob_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="loopjob_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
            ],
            options={
                "verbose_name": "Loop Job",
                "verbose_name_plural": "Loop Jobs",
                "db_table": "loop_jobs",
                "ordering": ("-created_at",),
            },
        ),
        migrations.CreateModel(
            name="LoopTarget",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("next_run_at", models.DateTimeField(blank=True, null=True)),
                ("last_skipped_at", models.DateTimeField(blank=True, null=True)),
                (
                    "last_skip_reason",
                    models.CharField(
                        blank=True,
                        choices=[
                            ("user_disabled", "User disabled this job"),
                            ("master_paused", "User paused all Auto PM"),
                            ("min_role", "Below the job's minimum role"),
                            ("llm_config_missing", "No usable LLM credentials"),
                            ("membership_gone", "No active workspace membership"),
                            ("turn_active", "Previous run still in flight"),
                            ("dispatch_error", "Unexpected error creating the turn"),
                        ],
                        default="",
                        max_length=64,
                    ),
                ),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="looptarget_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="looptarget_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "job",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="targets",
                        to="db.loopjob",
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="loop_targets",
                        to="db.workspace",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="loop_targets",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "thread",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="+",
                        to="assistant.assistantthread",
                    ),
                ),
                (
                    "last_run",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="+",
                        to="assistant.assistantturn",
                    ),
                ),
            ],
            options={
                "verbose_name": "Loop Target",
                "verbose_name_plural": "Loop Targets",
                "db_table": "loop_targets",
                "ordering": ("-created_at",),
            },
        ),
        migrations.CreateModel(
            name="LoopUserPreference",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("enabled", models.BooleanField(default=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="loopuserpreference_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="loopuserpreference_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="loop_preferences",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "job",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="user_preferences",
                        to="db.loopjob",
                    ),
                ),
            ],
            options={
                "verbose_name": "Loop User Preference",
                "verbose_name_plural": "Loop User Preferences",
                "db_table": "loop_user_preferences",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="loopjob",
            constraint=models.UniqueConstraint(
                fields=("slug",),
                condition=models.Q(deleted_at__isnull=True),
                name="loop_job_unique_slug_when_active",
            ),
        ),
        migrations.AddConstraint(
            model_name="looptarget",
            constraint=models.UniqueConstraint(
                fields=("job", "workspace", "user"),
                condition=models.Q(deleted_at__isnull=True),
                name="loop_target_unique_edge_when_active",
            ),
        ),
        migrations.AddIndex(
            model_name="looptarget",
            index=models.Index(fields=("next_run_at",), name="loop_target_due_idx"),
        ),
        migrations.AddConstraint(
            model_name="loopuserpreference",
            constraint=models.UniqueConstraint(
                fields=("user", "job"),
                condition=models.Q(deleted_at__isnull=True),
                name="loop_pref_unique_user_job_when_active",
            ),
        ),
        migrations.AddConstraint(
            model_name="loopuserpreference",
            constraint=models.UniqueConstraint(
                fields=("user",),
                condition=models.Q(job__isnull=True, deleted_at__isnull=True),
                name="loop_pref_unique_user_master_when_active",
            ),
        ),
        migrations.RunPython(seed_builtin_jobs, unseed_builtin_jobs),
    ]
