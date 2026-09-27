# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill ``AgentRun.failure_reason`` for existing failed rows (PDASHOSS01-183).

Runs the same classifier the write path uses (``pi_dash.runner.failure.
classify``) over the persisted ``error`` text and ``error_code``, so old
rows land in the same buckets new failures will. Importing live code in a
data migration is a deliberate trade here: the classifier is pure
(taxonomy strings + compiled regexes, no model access), and drifting from
the write-path rules would defeat the point of the backfill. If the module
moves, this migration must keep classifying — do not swap the import for a
frozen copy without re-checking bucket parity.
"""

from django.db import migrations

BATCH_SIZE = 1000


def backfill_failure_reason(apps, schema_editor):
    from pi_dash.runner.failure import classify

    AgentRun = apps.get_model("runner", "AgentRun")
    queryset = AgentRun.objects.filter(status="failed", failure_reason="").only(
        "id", "error", "error_code"
    )
    batch = []
    for run in queryset.iterator(chunk_size=BATCH_SIZE):
        run.failure_reason = classify(run.error or "", error_code=run.error_code or "").value
        batch.append(run)
        if len(batch) >= BATCH_SIZE:
            AgentRun.objects.bulk_update(batch, ["failure_reason"])
            batch = []
    if batch:
        AgentRun.objects.bulk_update(batch, ["failure_reason"])


def clear_failure_reason(apps, schema_editor):
    AgentRun = apps.get_model("runner", "AgentRun")
    AgentRun.objects.exclude(failure_reason="").update(failure_reason="")


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0030_agentrun_failure_reason"),
    ]

    operations = [
        migrations.RunPython(backfill_failure_reason, clear_failure_reason),
    ]
