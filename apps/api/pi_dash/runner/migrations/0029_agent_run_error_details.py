# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fold the failure trio into one ``error_details`` JSON column (PDASHOSS01-187).

``agent_run`` described one concept — how the run went wrong — across three
columns: ``error_code`` (indexed), ``error`` and ``refusal_category`` (blank on
every run that is not a refusal). They are written together at terminal time by
one writer and none was ever a query predicate, so the ``error_code`` index was
one the database maintained on every write and no query used.

They become ``{"code": …, "message": …, "refusal_category": …}``. Only keys with
a value are present, so a clean run carries ``{}`` and the model's ``error_code``
/ ``error`` properties read an absent key back as ``""`` — the API keeps emitting
the three flat keys unchanged.

``refusal_category`` keeps a real column, as a Postgres STORED generated column
derived from the JSON (the "hybrid" shape migration 0028 introduced for token
usage). Migration 0016 added it so a policy decline stays queryable apart from a
crash; folding it into JSON alone would have traded that for an expression index
the planner estimates badly. This keeps both.

No other column moves: the row is a state machine several writers advance by
compare-and-swap against real columns, and its ten composite indexes stay.

The reverse migration copies the three values back out.
"""

from django.db import migrations, models

import pi_dash.runner.fields

_BACKFILL = """
UPDATE agent_run
SET error_details = jsonb_strip_nulls(
    jsonb_build_object(
        'code', NULLIF(error_code, ''),
        'message', NULLIF(error, ''),
        'refusal_category', NULLIF(refusal_category, '')
    )
)
WHERE error_code <> '' OR error <> '' OR refusal_category <> '';
"""

_RESTORE = """
UPDATE agent_run
SET error_code = COALESCE(error_details ->> 'code', ''),
    error = COALESCE(error_details ->> 'message', ''),
    refusal_category = COALESCE(error_details ->> 'refusal_category', '');
"""


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0028_agent_run_usage_json"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="error_details",
            field=models.JSONField(blank=True, default=dict),
        ),
        migrations.RunSQL(sql=_BACKFILL, reverse_sql=_RESTORE),
        # Drops the unused ``agent_run.error_code`` btree index along with its
        # column.
        migrations.RemoveField(model_name="agentrun", name="error_code"),
        migrations.RemoveField(model_name="agentrun", name="error"),
        migrations.RemoveField(model_name="agentrun", name="refusal_category"),
        migrations.AddField(
            model_name="agentrun",
            name="refusal_category",
            field=pi_dash.runner.fields.JSONKeyTextField(key="refusal_category", source="error_details"),
        ),
    ]
