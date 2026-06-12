# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Refresh the global default and review PromptTemplates after the
run-trigger context addition.

The templates now render a "Why this run started" section from the new
``run.trigger`` / ``tick`` context keys (what dispatched the run — tick,
human comment, Run AI, state transition — plus the ticking cadence and
remaining budget). ``seed_*_template`` only fires with ``force=False``
from ``post_migrate``, so without this migration existing instances keep
serving the old bodies and agents never learn why they were invoked.

Both seed functions are no-ops when the stored body already equals the
assembled source, so re-running is safe.
"""

from __future__ import annotations

from django.db import migrations


def reseed_templates(apps, schema_editor):
    # Lazy import — the migration framework's app-loading should not
    # pull in unrelated modules at definition time.
    from pi_dash.prompting.seed import seed_default_template, seed_review_template

    seed_default_template(force=True)
    seed_review_template(force=True)


def noop_reverse(apps, schema_editor):
    # No reverse — the prior bodies are not preserved. The new context
    # keys are additive, so an old body renders fine either way.
    return


class Migration(migrations.Migration):
    dependencies = [
        ("prompting", "0003_reseed_default_workpad_template"),
    ]

    operations = [
        migrations.RunPython(reseed_templates, reverse_code=noop_reverse),
    ]
