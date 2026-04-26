# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub Issue Sync — MVP schema.

Adds operational fields to GithubRepositorySync, a metadata JSON to
GithubIssueSync (used for completion-comment idempotency, upstream-gone flag,
GitHub author identity), separate GitHub-side timestamps to avoid fighting
TimeAuditModel's auto_now_add/auto_now, and a one-binding-per-project
constraint that the existing unique_together [project, repository] could not
enforce.

See .ai_design/github_sync/design.md §5.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0126_issue_assigned_pod"),
    ]

    operations = [
        migrations.AddField(
            model_name="githubrepositorysync",
            name="is_sync_enabled",
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name="githubrepositorysync",
            name="last_synced_at",
            field=models.DateTimeField(null=True, blank=True),
        ),
        migrations.AddField(
            model_name="githubrepositorysync",
            name="last_sync_error",
            field=models.TextField(blank=True, default=""),
        ),
        migrations.AddField(
            model_name="githubissuesync",
            name="metadata",
            field=models.JSONField(default=dict),
        ),
        migrations.AddField(
            model_name="githubissuesync",
            name="gh_issue_created_at",
            field=models.DateTimeField(null=True, blank=True),
        ),
        migrations.AddField(
            model_name="githubissuesync",
            name="gh_issue_updated_at",
            field=models.DateTimeField(null=True, blank=True),
        ),
        migrations.AddConstraint(
            model_name="githubrepositorysync",
            constraint=models.UniqueConstraint(
                fields=["project"],
                condition=models.Q(deleted_at__isnull=True),
                name="github_repository_sync_unique_per_project_when_active",
            ),
        ),
    ]
