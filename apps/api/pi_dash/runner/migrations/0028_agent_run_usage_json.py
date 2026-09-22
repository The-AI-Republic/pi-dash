# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fold token usage into one ``usage`` JSON column (PDASHOSS01-188).

``agent_run`` and ``runner_live_state`` each carried three nullable bigint
columns — ``input_tokens`` / ``output_tokens`` / ``total_tokens`` — with no
room for cache or reasoning counters. Both tables now store a ``usage`` JSON
bag in the canonical shape of ``pi_dash.runner.services.usage``.

``agent_run`` keeps the three names as Postgres STORED generated columns
derived from ``usage``: workspace / project analytics ``Sum`` them, so they
stay real columns with real statistics (the "hybrid" shape). Nothing
aggregates ``runner_live_state``, so there the flat names become model
properties and the columns go.

Existing values are copied into ``usage`` before the old columns are dropped;
a counter that was NULL stays absent, so historical runs read back exactly
the numbers they had. The reverse migration copies them back out.
"""

from django.db import migrations, models

import pi_dash.runner.fields

_TABLES = ("agent_run", "runner_live_state")

_BACKFILL = """
UPDATE {table}
SET usage = jsonb_strip_nulls(
    jsonb_build_object('input', input_tokens, 'output', output_tokens, 'total', total_tokens)
)
WHERE input_tokens IS NOT NULL OR output_tokens IS NOT NULL OR total_tokens IS NOT NULL;
"""

_RESTORE = """
UPDATE {table}
SET input_tokens = (usage ->> 'input')::bigint,
    output_tokens = (usage ->> 'output')::bigint,
    total_tokens = (usage ->> 'total')::bigint;
"""


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0027_agentrun_trigger_blocker_completed"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="usage",
            field=models.JSONField(blank=True, default=dict),
        ),
        migrations.AddField(
            model_name="runnerlivestate",
            name="usage",
            field=models.JSONField(blank=True, default=dict),
        ),
        migrations.RunSQL(
            sql=[_BACKFILL.format(table=table) for table in _TABLES],
            reverse_sql=[_RESTORE.format(table=table) for table in _TABLES],
        ),
        *[
            migrations.RemoveField(model_name=model_name, name=name)
            for model_name in ("agentrun", "runnerlivestate")
            for name in ("input_tokens", "output_tokens", "total_tokens")
        ],
        *[
            migrations.AddField(
                model_name="agentrun",
                name=f"{key}_tokens",
                field=pi_dash.runner.fields.JSONKeyBigIntegerField(key=key, source="usage"),
            )
            for key in ("input", "output", "total")
        ],
    ]
