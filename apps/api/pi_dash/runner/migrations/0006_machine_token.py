# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Multi-runner foundation: introduce MachineToken and a nullable FK on
Runner so a daemon can authenticate as a token (machine credential) and
the runner records under it can be discovered without each carrying its
own bearer secret. See ``.ai_design/n_runners_in_same_machine/design.md``
§5.1 and tasks.md §2.1.

The Runner.machine_token FK is nullable — legacy v1 installs (no token)
keep working unchanged until the operator runs ``pidash configure token``
and the cloud's Phase 2 attach endpoint links the runner.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0001_initial"),
        ("runner", "0005_paused_status_and_pinned_runner"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="MachineToken",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("title", models.CharField(max_length=128)),
                ("secret_hash", models.CharField(db_index=True, max_length=128)),
                ("secret_fingerprint", models.CharField(max_length=16)),
                ("last_seen_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "revoked_at",
                    models.DateTimeField(blank=True, db_index=True, null=True),
                ),
                (
                    "created_by",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="machine_tokens",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="machine_tokens",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "machine_token",
                "ordering": ("-created_at",),
                "indexes": [
                    models.Index(
                        fields=["workspace", "revoked_at"],
                        name="machine_tok_workspc_revoked_idx",
                    ),
                    models.Index(
                        fields=["created_by", "revoked_at"],
                        name="machine_tok_creator_revoked_idx",
                    ),
                ],
            },
        ),
        migrations.AddField(
            model_name="runner",
            name="machine_token",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="runners",
                to="runner.machinetoken",
            ),
        ),
    ]
