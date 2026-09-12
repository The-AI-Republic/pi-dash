# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Debounced state-transition dispatch.

Adds ``IssuePendingDispatch`` — one row per issue holding the pending
(debounced) dispatch intent: a monotonic ``token``, the target fire time,
and the transition's source/target states so the delayed job can resolve
the cross-phase session shape.

See ``.ai_design/state_transition_debounce/design.md``.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0162_merge_complexity_and_profile_settings"),
    ]

    operations = [
        migrations.CreateModel(
            name="IssuePendingDispatch",
            fields=[
                (
                    "created_at",
                    models.DateTimeField(auto_now_add=True, verbose_name="Created At"),
                ),
                (
                    "updated_at",
                    models.DateTimeField(auto_now=True, verbose_name="Last Modified At"),
                ),
                (
                    "deleted_at",
                    models.DateTimeField(blank=True, null=True, verbose_name="Deleted At"),
                ),
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
                ("token", models.BigIntegerField(default=0)),
                ("dispatch_at", models.DateTimeField(blank=True, null=True)),
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
                    "issue",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="pending_dispatch",
                        to="db.issue",
                    ),
                ),
                (
                    "from_state",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="+",
                        to="db.state",
                    ),
                ),
                (
                    "to_state",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="+",
                        to="db.state",
                    ),
                ),
            ],
            options={
                "verbose_name": "Issue Pending Dispatch",
                "verbose_name_plural": "Issue Pending Dispatches",
                "db_table": "issue_pending_dispatch",
            },
        ),
    ]
