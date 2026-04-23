# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Idempotently ensure every workspace has a default pod.

Intended for dev / test environments where workspaces may have been created
before the post_save signal was wired in. On a fresh deploy with no existing
workspaces, this command is a no-op.

Usage::

    python manage.py ensure_workspace_pods
    python manage.py ensure_workspace_pods --dry-run

See ``.ai_design/issue_runner/design.md`` §9.3.
"""

from __future__ import annotations

from django.core.management.base import BaseCommand
from django.db import transaction

from pi_dash.db.models.workspace import Workspace
from pi_dash.runner.models import Pod


class Command(BaseCommand):
    help = "Ensure every workspace has a default pod; creates missing pods idempotently."

    def add_arguments(self, parser):
        parser.add_argument(
            "--dry-run",
            action="store_true",
            help="Report what would change without writing anything.",
        )

    def handle(self, *args, dry_run: bool = False, **options):
        missing = []
        for ws in Workspace.objects.all():
            if Pod.objects.filter(workspace=ws).exists():
                continue
            missing.append(ws)

        if not missing:
            self.stdout.write(self.style.SUCCESS("All workspaces already have a pod. Nothing to do."))
            return

        self.stdout.write(
            f"Found {len(missing)} workspace(s) without a pod:"
        )
        for ws in missing:
            self.stdout.write(f"  - {ws.id} ({ws.name})")

        if dry_run:
            self.stdout.write(self.style.WARNING("--dry-run: no changes written."))
            return

        with transaction.atomic():
            for ws in missing:
                Pod.objects.create(
                    workspace=ws,
                    name=f"{ws.name}-pod",
                    description="Auto-created default pod by ensure_workspace_pods.",
                    is_default=True,
                    created_by=getattr(ws, "owner", None),
                )
        self.stdout.write(
            self.style.SUCCESS(f"Created {len(missing)} default pod(s).")
        )
