# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dev-machine run-queue columns missing from the OSS schema (NEWPIDASH-372).

The Rust entities (``rust-api/crates/db/src/entity/agent_run.rs`` and
``runner_session.rs``) declare ``agent_run.assignment_token``,
``agent_run.assignment_protocol``, ``agent_run.capacity_wait`` and
``runner_session.capabilities`` (NEWPIDASH-241), but no OSS migration
created them, so every full-model ``agent_run`` select fails with Postgres
42703 on a database migrated from main.

Cloud already has these columns via
``rust-cloud/crates/cloud-migration/sql/native_runner_0040.sql``, so this
migration adopts them idempotently (``ADD COLUMN IF NOT EXISTS`` plus a
guarded ``DO`` block for the check constraint). Schema-only: no Python
behaviour. The DDL matches ``native_runner_0040.sql`` exactly so cloud and
OSS schemas stay identical, and the NOT NULL columns keep persistent
database defaults (the ``db/0174`` / ``db/0181`` pattern) so older writers
that omit the columns keep working.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0043_desktop_handoff"),
    ]

    operations = [
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql='ALTER TABLE "agent_run" ADD COLUMN IF NOT EXISTS "assignment_token" uuid NULL',
                    reverse_sql='ALTER TABLE "agent_run" DROP COLUMN IF EXISTS "assignment_token"',
                ),
            ],
            state_operations=[
                migrations.AddField(
                    model_name="agentrun",
                    name="assignment_token",
                    field=models.UUIDField(blank=True, null=True),
                ),
            ],
        ),
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql=(
                        'ALTER TABLE "agent_run" ADD COLUMN IF NOT EXISTS'
                        ' "assignment_protocol" smallint NOT NULL DEFAULT 0'
                    ),
                    reverse_sql='ALTER TABLE "agent_run" DROP COLUMN IF EXISTS "assignment_protocol"',
                ),
            ],
            state_operations=[
                migrations.AddField(
                    model_name="agentrun",
                    name="assignment_protocol",
                    field=models.SmallIntegerField(default=0),
                ),
            ],
        ),
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql='ALTER TABLE "agent_run" ADD COLUMN IF NOT EXISTS "capacity_wait" jsonb NULL',
                    reverse_sql='ALTER TABLE "agent_run" DROP COLUMN IF EXISTS "capacity_wait"',
                ),
            ],
            state_operations=[
                migrations.AddField(
                    model_name="agentrun",
                    name="capacity_wait",
                    field=models.JSONField(blank=True, null=True),
                ),
            ],
        ),
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql=(
                        'ALTER TABLE "runner_session" ADD COLUMN IF NOT EXISTS'
                        " \"capabilities\" jsonb NOT NULL DEFAULT '[]'"
                    ),
                    reverse_sql='ALTER TABLE "runner_session" DROP COLUMN IF EXISTS "capabilities"',
                ),
            ],
            state_operations=[
                migrations.AddField(
                    model_name="runnersession",
                    name="capabilities",
                    field=models.JSONField(blank=True, default=list),
                ),
            ],
        ),
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql="""DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_run_assignment_protocol_check'
    ) THEN
        ALTER TABLE "agent_run"
            ADD CONSTRAINT "agent_run_assignment_protocol_check"
            CHECK ("assignment_protocol" IN (0, 1));
    END IF;
END
$$""",
                    reverse_sql='ALTER TABLE "agent_run" DROP CONSTRAINT IF EXISTS "agent_run_assignment_protocol_check"',
                ),
            ],
            state_operations=[
                migrations.AddConstraint(
                    model_name="agentrun",
                    constraint=models.CheckConstraint(
                        check=models.Q(assignment_protocol__in=[0, 1]),
                        name="agent_run_assignment_protocol_check",
                    ),
                ),
            ],
        ),
        migrations.RunSQL(
            sql=[
                'ALTER TABLE "agent_run" ALTER COLUMN "assignment_protocol" SET DEFAULT 0',
                'ALTER TABLE "runner_session" ALTER COLUMN "capabilities" SET DEFAULT \'[]\'',
            ],
            reverse_sql=[
                'ALTER TABLE "agent_run" ALTER COLUMN "assignment_protocol" DROP DEFAULT',
                'ALTER TABLE "runner_session" ALTER COLUMN "capabilities" DROP DEFAULT',
            ],
        ),
    ]
