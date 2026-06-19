# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0150_github_app_foundation"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="GithubPullRequestLink",
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
                ("repo_owner", models.CharField(max_length=255)),
                ("repo_name", models.CharField(max_length=255)),
                ("pr_number", models.PositiveIntegerField()),
                ("url", models.URLField(max_length=500)),
                ("title", models.CharField(blank=True, default="", max_length=500)),
                (
                    "state",
                    models.CharField(
                        choices=[("open", "Open"), ("closed", "Closed")],
                        default="open",
                        max_length=12,
                    ),
                ),
                ("merged", models.BooleanField(default=False)),
                ("draft", models.BooleanField(default=False)),
                ("pr_updated_at", models.DateTimeField(blank=True, null=True)),
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
                    "issue",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="github_pull_requests",
                        to="db.issue",
                    ),
                ),
                (
                    "project",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="project_%(class)s",
                        to="db.project",
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
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="workspace_%(class)s",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "Github Pull Request Link",
                "verbose_name_plural": "Github Pull Request Links",
                "db_table": "github_pull_request_links",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddIndex(
            model_name="githubpullrequestlink",
            index=models.Index(
                fields=["repo_owner", "repo_name", "pr_number"],
                name="github_pr_l_repo_ow_idx",
            ),
        ),
        migrations.AddIndex(
            model_name="githubpullrequestlink",
            index=models.Index(fields=["issue"], name="github_pr_l_issue_idx"),
        ),
        migrations.AddConstraint(
            model_name="githubpullrequestlink",
            constraint=models.UniqueConstraint(
                condition=models.Q(("deleted_at__isnull", True)),
                fields=("repo_owner", "repo_name", "pr_number"),
                name="github_pr_link_unique_per_pr_when_active",
            ),
        ),
    ]
