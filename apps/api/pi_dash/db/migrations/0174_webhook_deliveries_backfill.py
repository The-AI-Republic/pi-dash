# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-357 (github_* sunset 4/6): backfill post-0152 webhook deliveries.
# Migration 0152 copied github_webhook_deliveries into git_webhook_deliveries once,
# but deliveries created after 0152 still live only in the legacy table.
# This migration is idempotent: matched on (provider, host_url, delivery_id)
# and on the legacy metadata key (metadata.legacy_installation_id). It copies
# missing rows from github_webhook_deliveries into git_webhook_deliveries with
# provider='github' and host_url='https://github.com'.
#
# It is safe to run more than once: existing generic rows are untouched.
# The Rust and Django webhook handlers now write only to git_webhook_deliveries;
# legacy rows remain for audit but receive no new writes.

from django.db import migrations


GITHUB_HOST = "https://github.com"


def backfill_webhook_deliveries(apps, schema_editor):
    GitWebhookDelivery = apps.get_model("db", "GitWebhookDelivery")
    GithubWebhookDelivery = apps.get_model("db", "GithubWebhookDelivery")

    for old in GithubWebhookDelivery.objects.all().iterator(chunk_size=1000):
        delivery_str = str(old.delivery_id)
        # Existence check on the generic unique key (provider/host/delivery_id).
        if GitWebhookDelivery.objects.filter(
            provider="github",
            host_url=GITHUB_HOST,
            delivery_id=delivery_str,
            deleted_at__isnull=True,
        ).exists():
            continue
        metadata = {}
        if getattr(old, "installation_id", None) is not None:
            metadata["legacy_installation_id"] = old.installation_id
        GitWebhookDelivery.objects.create(
            provider="github",
            host_url=GITHUB_HOST,
            delivery_id=delivery_str,
            event=old.event or "",
            action=old.action or "",
            raw_headers={},
            payload=old.payload or {},
            status=old.status,
            received_at=old.received_at,
            processed_at=old.processed_at,
            error=old.error or "",
            metadata=metadata,
            created_by_id=old.created_by_id,
            updated_by_id=old.updated_by_id,
        )


def noop_reverse(apps, schema_editor):
    return None


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0172_clidevicecode_engine_scope"),
    ]

    operations = [
        migrations.RunPython(backfill_webhook_deliveries, noop_reverse),
    ]
