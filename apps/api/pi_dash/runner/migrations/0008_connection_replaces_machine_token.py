# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Reshape runner registration around a first-class Connection model.

Drops the legacy single-runner-secret auth path and folds the existing
``MachineToken`` concept into a renamed ``Connection`` model that is
the user-facing primitive in the cloud UI. Adds a one-time enrollment
token that the daemon exchanges for the long-lived ``connection_secret``.

The migration is destructive — runner / connection / run / event /
approval rows are all truncated. The user invoking this migration has
opted into "no data migration, just remove old data" (dev iteration).
"""

from __future__ import annotations

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0007_pod_project_relationship"),
    ]

    operations = [
        # 1. Wipe data from every table that references the dropped fields.
        #    Order doesn't matter under TRUNCATE ... CASCADE; we list the
        #    leaves first for readability. RunSQL.noop on reverse — there's
        #    no meaningful undo for a TRUNCATE.
        migrations.RunSQL(
            sql=(
                "TRUNCATE TABLE "
                "agent_run_approval, agent_run_event, agent_run, "
                "runner_registration_token, runner, machine_token "
                "RESTART IDENTITY CASCADE;"
            ),
            reverse_sql=migrations.RunSQL.noop,
        ),
        # 2. Drop the one-time runner registration token model entirely —
        #    the new flow uses Connection.enrollment_token_* instead.
        migrations.DeleteModel(name="RunnerRegistrationToken"),
        # 3. Drop the per-runner bearer credential. Daemons authenticate
        #    via the Connection now; runner_id is just a routing key.
        migrations.RemoveField(model_name="runner", name="credential_hash"),
        migrations.RemoveField(model_name="runner", name="credential_fingerprint"),
        # 4. Rename MachineToken → Connection. Renames the model and its
        #    db_table in one go (RenameModel auto-renames the table when
        #    db_table is defaulted; we set it explicitly below for safety).
        migrations.RenameModel(old_name="MachineToken", new_name="Connection"),
        migrations.AlterModelTable(name="connection", table="connection"),
        # 5. Rename the FK on Runner: machine_token → connection. Then
        #    AlterField to make it non-null + CASCADE on delete.
        migrations.RenameField(
            model_name="runner",
            old_name="machine_token",
            new_name="connection",
        ),
        migrations.AlterField(
            model_name="runner",
            name="connection",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.CASCADE,
                related_name="runners",
                to="runner.connection",
            ),
        ),
        # 6. Reshape Connection: drop title, add name + host_label +
        #    enrollment_* + enrolled_at. secret_hash / secret_fingerprint
        #    become blank-allowed since they're empty until enrollment.
        migrations.RemoveField(model_name="connection", name="title"),
        migrations.AddField(
            model_name="connection",
            name="name",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="connection",
            name="host_label",
            field=models.CharField(blank=True, default="", max_length=255),
        ),
        migrations.AddField(
            model_name="connection",
            name="enrollment_token_hash",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="connection",
            name="enrollment_token_fingerprint",
            field=models.CharField(blank=True, default="", max_length=16),
        ),
        migrations.AddField(
            model_name="connection",
            name="enrolled_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AlterField(
            model_name="connection",
            name="secret_hash",
            field=models.CharField(blank=True, db_index=True, default="", max_length=128),
        ),
        migrations.AlterField(
            model_name="connection",
            name="secret_fingerprint",
            field=models.CharField(blank=True, default="", max_length=16),
        ),
        # 7. Rename related_names on FKs to match the new model name.
        migrations.AlterField(
            model_name="connection",
            name="workspace",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.CASCADE,
                related_name="connections",
                to="db.workspace",
            ),
        ),
        migrations.AlterField(
            model_name="connection",
            name="created_by",
            field=models.ForeignKey(
                on_delete=django.db.models.deletion.CASCADE,
                related_name="connections",
                to=settings.AUTH_USER_MODEL,
            ),
        ),
        # 8. Unique-name-per-workspace among non-revoked connections.
        migrations.AddConstraint(
            model_name="connection",
            constraint=models.UniqueConstraint(
                condition=models.Q(("revoked_at__isnull", True)),
                fields=("workspace", "name"),
                name="connection_unique_name_per_workspace_when_active",
            ),
        ),
    ]
