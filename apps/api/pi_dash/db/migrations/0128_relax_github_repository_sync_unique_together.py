# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop GithubRepositorySync unique_together = [project, repository].

The constraint had no `deleted_at IS NULL` condition, so a soft-deleted
binding row blocked rebinding the same project to the same repository
(the new active row would collide with the soft-deleted row's
`(project, repository)` pair). The
`github_repository_sync_unique_per_project_when_active` constraint added
in 0127 already enforces the operational invariant ("at most one active
binding per project") and is correctly filtered on `deleted_at`, so the
old `unique_together` is now redundant *and* harmful.

See PR #65 review (Gap B). No data change; constraint-only migration.
"""

from django.db import migrations


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0127_github_sync_mvp"),
    ]

    operations = [
        migrations.AlterUniqueTogether(
            name="githubrepositorysync",
            unique_together=set(),
        ),
    ]
