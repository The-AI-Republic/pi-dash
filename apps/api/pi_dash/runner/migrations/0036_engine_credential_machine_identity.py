# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Engine credential, daemon-credential expiry and machine identity.

PIDESKAPP-122, ``.ai_design/built_engine_as_runner/design.md`` §4.3, §4.9,
§4.10 and §13:

- ``EngineCredential``: one live per machine (partial unique constraint),
  fixed 14-day lifetime, used only to obtain model tokens.
- ``DevMachine.engine_sign_in_state`` / ``engine_credential_expires_at``:
  the machine's engine sign-in, mirrored from its credential.
- ``DevMachine.engine_state`` / ``engine_state_reported_at``: the engine
  state the daemon reports in its machine-session heartbeat.
- ``DevMachine.merged_into`` / ``merge_result``: machine merge (§13).
- ``MachineSession.machine_token``: the token a session was opened with.
- ``MachineToken.expires_at``: 60-day idle expiry. The data migration that
  follows backfills existing rows.

Schema only and additive.
"""

import django.db.models.deletion
import uuid
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("runner", "0035_engine_dispatch_data"),
    ]

    operations = [
        migrations.AddField(
            model_name="devmachine",
            name="engine_sign_in_state",
            field=models.CharField(
                choices=[
                    ("never", "Never signed in"),
                    ("active", "Signed in"),
                    ("expiring", "Expiring soon"),
                    ("expired", "Expired"),
                    ("signed_out", "Signed out"),
                    ("revoked", "Revoked"),
                ],
                default="never",
                max_length=16,
            ),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_credential_expires_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_state",
            field=models.JSONField(blank=True, default=dict),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_state_reported_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="merged_into",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="merged_from",
                to="runner.devmachine",
            ),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="merge_result",
            field=models.JSONField(blank=True, default=dict),
        ),
        migrations.AddField(
            model_name="machinetoken",
            name="expires_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="machinesession",
            name="machine_token",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="machine_sessions",
                to="runner.machinetoken",
            ),
        ),
        migrations.CreateModel(
            name="EngineCredential",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("token_hash", models.CharField(max_length=128, unique=True)),
                ("fingerprint", models.CharField(blank=True, default="", max_length=16)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("expires_at", models.DateTimeField()),
                ("last_used_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                (
                    "revoked_reason",
                    models.CharField(
                        blank=True,
                        choices=[
                            ("superseded", "Replaced by a newer credential"),
                            ("signed_out", "Signed out by the user"),
                            ("machine_removed", "Computer removed"),
                            ("machine_merged", "Computer merged into another"),
                            ("user_signed_out", "User signed out of all sessions"),
                            ("admin", "Revoked by an administrator"),
                        ],
                        default="",
                        max_length=32,
                    ),
                ),
                (
                    "dev_machine",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="engine_credentials",
                        to="runner.devmachine",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="engine_credentials",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "engine_credential",
                "ordering": ("-created_at",),
                "indexes": [
                    models.Index(fields=["user", "revoked_at"], name="engine_cred_user_rev_idx"),
                ],
                "constraints": [
                    models.UniqueConstraint(
                        condition=models.Q(("revoked_at__isnull", True)),
                        fields=("dev_machine",),
                        name="engine_credential_one_live_per_machine",
                    ),
                ],
            },
        ),
        # Django drops column defaults after backfilling. Keep database-level
        # defaults so writers that bypass the Django model (the Rust API, raw
        # SQL) can insert rows without naming the new NOT NULL columns, and
        # so a Rust-minted machine token still gets the 60-day idle expiry.
        migrations.RunSQL(
            sql=[
                "ALTER TABLE dev_machine ALTER COLUMN engine_sign_in_state SET DEFAULT 'never'",
                "ALTER TABLE dev_machine ALTER COLUMN engine_state SET DEFAULT '{}'::jsonb",
                "ALTER TABLE dev_machine ALTER COLUMN merge_result SET DEFAULT '{}'::jsonb",
                "ALTER TABLE machine_token ALTER COLUMN expires_at SET DEFAULT (now() + interval '60 days')",
                "ALTER TABLE engine_credential ALTER COLUMN fingerprint SET DEFAULT ''",
                "ALTER TABLE engine_credential ALTER COLUMN revoked_reason SET DEFAULT ''",
                "ALTER TABLE engine_credential ALTER COLUMN created_at SET DEFAULT now()",
            ],
            reverse_sql=[
                "ALTER TABLE dev_machine ALTER COLUMN engine_sign_in_state DROP DEFAULT",
                "ALTER TABLE dev_machine ALTER COLUMN engine_state DROP DEFAULT",
                "ALTER TABLE dev_machine ALTER COLUMN merge_result DROP DEFAULT",
                "ALTER TABLE machine_token ALTER COLUMN expires_at DROP DEFAULT",
                "ALTER TABLE engine_credential ALTER COLUMN fingerprint DROP DEFAULT",
                "ALTER TABLE engine_credential ALTER COLUMN revoked_reason DROP DEFAULT",
                "ALTER TABLE engine_credential ALTER COLUMN created_at DROP DEFAULT",
            ],
        ),
    ]
