# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


def backfill_dev_machines(apps, schema_editor):
    DevMachine = apps.get_model("runner", "DevMachine")
    MachineToken = apps.get_model("runner", "MachineToken")
    Runner = apps.get_model("runner", "Runner")
    db_alias = schema_editor.connection.alias
    cache = {}

    def machine_for(owner_id, host_label, seen_at=None):
        host_label = (host_label or "").strip()[:255]
        if not owner_id or not host_label:
            return None
        key = (owner_id, host_label)
        machine = cache.get(key)
        if machine is None:
            machine = DevMachine.objects.using(db_alias).create(
                owner_id=owner_id,
                host_label=host_label,
                label=host_label[:128],
                visibility=0,
                last_seen_at=seen_at,
            )
            cache[key] = machine
        elif seen_at and (machine.last_seen_at is None or machine.last_seen_at < seen_at):
            machine.last_seen_at = seen_at
            machine.save(update_fields=["last_seen_at", "updated_at"])
        return machine

    for token in (
        MachineToken.objects.using(db_alias)
        .filter(dev_machine__isnull=True)
        .exclude(host_label="")
        .iterator()
    ):
        machine = machine_for(token.user_id, token.host_label, token.last_used_at)
        if machine is not None:
            MachineToken.objects.using(db_alias).filter(pk=token.pk).update(dev_machine_id=machine.id)

    for runner in (
        Runner.objects.using(db_alias)
        .filter(dev_machine__isnull=True)
        .exclude(host_label="")
        .iterator()
    ):
        machine = machine_for(runner.owner_id, runner.host_label, runner.last_heartbeat_at)
        if machine is not None:
            Runner.objects.using(db_alias).filter(pk=runner.pk).update(dev_machine_id=machine.id)


def reverse_backfill_dev_machines(apps, schema_editor):
    DevMachine = apps.get_model("runner", "DevMachine")
    MachineToken = apps.get_model("runner", "MachineToken")
    Runner = apps.get_model("runner", "Runner")
    db_alias = schema_editor.connection.alias
    Runner.objects.using(db_alias).update(dev_machine_id=None)
    MachineToken.objects.using(db_alias).update(dev_machine_id=None)
    DevMachine.objects.using(db_alias).all().delete()


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("runner", "0014_agent_run_usage"),
    ]

    operations = [
        migrations.CreateModel(
            name="DevMachine",
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
                ("host_label", models.CharField(blank=True, default="", max_length=255)),
                ("label", models.CharField(blank=True, default="", max_length=128)),
                (
                    "visibility",
                    models.PositiveSmallIntegerField(choices=[(0, "Private")], db_index=True, default=0),
                ),
                ("last_seen_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "owner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="dev_machines",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "dev_machine",
                "ordering": ("-last_seen_at", "-created_at"),
            },
        ),
        migrations.AddField(
            model_name="runner",
            name="visibility",
            field=models.PositiveSmallIntegerField(choices=[(0, "Private")], db_index=True, default=0),
        ),
        migrations.AddField(
            model_name="runner",
            name="dev_machine",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="runners",
                to="runner.devmachine",
            ),
        ),
        migrations.RemoveConstraint(
            model_name="machinetoken",
            name="machine_token_one_active_per_user_ws_host",
        ),
        migrations.AddField(
            model_name="machinetoken",
            name="dev_machine",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="machine_tokens",
                to="runner.devmachine",
            ),
        ),
        migrations.RunPython(backfill_dev_machines, reverse_backfill_dev_machines),
        migrations.AddIndex(
            model_name="devmachine",
            index=models.Index(fields=["owner", "visibility"], name="dev_machine_owner_vis_idx"),
        ),
        migrations.AddIndex(
            model_name="devmachine",
            index=models.Index(fields=["owner", "host_label"], name="dev_machine_owner_host_idx"),
        ),
        migrations.AddIndex(
            model_name="devmachine",
            index=models.Index(fields=["host_label"], name="dev_machine_host_idx"),
        ),
        migrations.AddIndex(
            model_name="runner",
            index=models.Index(
                fields=["dev_machine", "status"],
                name="runner_dev_machine_status_idx",
            ),
        ),
        migrations.AddIndex(
            model_name="machinetoken",
            index=models.Index(fields=["dev_machine", "revoked_at"], name="machine_token_dev_rev_idx"),
        ),
        migrations.AddConstraint(
            model_name="machinetoken",
            constraint=models.UniqueConstraint(
                condition=models.Q(("dev_machine__isnull", True), ("revoked_at__isnull", True)),
                fields=("user", "workspace", "host_label"),
                name="machine_token_one_active_per_user_ws_host",
            ),
        ),
        migrations.AddConstraint(
            model_name="machinetoken",
            constraint=models.UniqueConstraint(
                condition=models.Q(("dev_machine__isnull", False), ("revoked_at__isnull", True)),
                fields=("workspace", "dev_machine"),
                name="machine_token_one_active_per_ws_dev_machine",
            ),
        ),
    ]
