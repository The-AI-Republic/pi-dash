# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace join requests (request-to-join by admin email).

Creates ``WorkspaceJoinRequest`` — the inverse of ``WorkspaceMemberInvite``:
a signed-in but workspace-less user asks to join a workspace by typing a
workspace admin's email during onboarding, and a workspace admin approves or
denies it. ``workspace`` is nullable so an unresolved request (an email that
does not belong to any workspace admin) can still be recorded, keeping the
requester's onboarding "pending" state identical either way and avoiding an
admin-existence enumeration leak.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0152_git_generalization"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="WorkspaceJoinRequest",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                        unique=True,
                    ),
                ),
                ("admin_email", models.CharField(max_length=255)),
                ("message", models.TextField(blank=True, null=True)),
                (
                    "role",
                    models.PositiveSmallIntegerField(
                        choices=[(20, "Admin"), (15, "Member"), (5, "Guest")], default=15
                    ),
                ),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("PENDING", "Pending"),
                            ("APPROVED", "Approved"),
                            ("DENIED", "Denied"),
                        ],
                        default="PENDING",
                        max_length=20,
                    ),
                ),
                ("responded_at", models.DateTimeField(blank=True, null=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "requester",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="workspace_join_request",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "responded_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="responded_workspace_join_request",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="workspace_join_request",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "Workspace Join Request",
                "verbose_name_plural": "Workspace Join Requests",
                "db_table": "workspace_join_requests",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="workspacejoinrequest",
            constraint=models.UniqueConstraint(
                condition=models.Q(("deleted_at__isnull", True), ("status", "PENDING")),
                fields=("requester", "workspace"),
                name="workspace_join_request_unique_requester_workspace_when_pending",
            ),
        ),
    ]
