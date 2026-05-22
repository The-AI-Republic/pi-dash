# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop the implicit ``UNIQUE`` constraint on
``github_repository_syncs.repository_id``.

The constraint came from the field being declared as ``OneToOneField``
in the model. Like the old ``unique_together = [project, repository]``
that migration 0128 already dropped, it has no ``deleted_at IS NULL``
qualifier — so a soft-deleted binding row keeps the slot, blocking
rebind of the same project to the same repo.

The operational invariant ("at most one ACTIVE binding per project") is
already enforced by
``github_repository_sync_unique_per_project_when_active`` (added in
0127). This migration drops the now-redundant repository-level
uniqueness so unbind+rebind works without a manual constraint cleanup.

Field-only migration: the column stays, only the implicit unique index
goes away. Existing data is unaffected.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        # 0136 is the merge migration that joins `0135_clidevicecode`
        # (CLI device-code auth) with `0135_review_state_and_cadence_split`
        # (the "In Review" state). Depending on it ensures both prior
        # branches' changes are present before this one runs.
        ("db", "0136_merge_cli_device_code_and_review_state"),
    ]

    operations = [
        migrations.AlterField(
            model_name="githubrepositorysync",
            name="repository",
            field=models.ForeignKey(
                on_delete=models.deletion.CASCADE,
                related_name="syncs",
                to="db.githubrepository",
            ),
        ),
    ]
