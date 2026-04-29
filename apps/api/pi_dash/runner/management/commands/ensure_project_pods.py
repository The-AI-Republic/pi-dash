# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Idempotently ensure every project has a default pod.

Intended for dev / test environments where projects may have been created
before the post_save(Project) signal was wired in, or in CI fixtures that
disable signals. On a fresh deploy, the migration's required backfill
(``0007_pod_project_relationship``) covers the same ground.

Usage::

    python manage.py ensure_project_pods
    python manage.py ensure_project_pods --dry-run

See ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§6.1 and §11.
"""

from __future__ import annotations

from django.core.management.base import BaseCommand
from django.db import transaction

from pi_dash.db.models.project import Project
from pi_dash.runner.models import Pod


class Command(BaseCommand):
    help = "Ensure every project has a default pod; creates missing pods idempotently."

    def add_arguments(self, parser):
        parser.add_argument(
            "--dry-run",
            action="store_true",
            help="Report what would change without writing anything.",
        )

    def handle(self, *args, dry_run: bool = False, **options):
        missing = []
        for project in Project.objects.all():
            if Pod.objects.filter(project=project).exists():
                continue
            missing.append(project)

        if not missing:
            self.stdout.write(
                self.style.SUCCESS("All projects already have a pod. Nothing to do.")
            )
            return

        self.stdout.write(f"Found {len(missing)} project(s) without a pod:")
        for project in missing:
            self.stdout.write(f"  - {project.id} ({project.identifier})")

        if dry_run:
            self.stdout.write(self.style.WARNING("--dry-run: no changes written."))
            return

        # Each pod creation in its own small transaction with get_or_create so a
        # concurrent signal (or a second invocation of this command) races
        # safely. Without this an IntegrityError mid-batch would abort the
        # entire atomic block.
        created = 0
        for project in missing:
            pod_name = f"{project.identifier}_pod_1"
            with transaction.atomic():
                _, was_created = Pod.objects.get_or_create(
                    project=project,
                    name=pod_name,
                    defaults={
                        "workspace_id": project.workspace_id,
                        "description": "Auto-created default pod by ensure_project_pods.",
                        "is_default": True,
                        "created_by": getattr(project, "project_lead", None)
                        or getattr(project, "default_assignee", None),
                    },
                )
                if was_created:
                    created += 1

        self.stdout.write(self.style.SUCCESS(f"Created {created} default pod(s)."))
