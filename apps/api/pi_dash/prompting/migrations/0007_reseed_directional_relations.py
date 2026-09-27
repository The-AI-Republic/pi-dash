# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Refresh the global default PromptTemplate after the relationships section
gains directional relations.

``seed_default_template`` only fires with ``force=False`` from the
``post_migrate`` signal, so an existing instance keeps serving the
previously-seeded body even after fragment files change on disk. The
"Work item relationships" section now also lists blocked-by / blocking /
other directional relations and carries the open-blocker decision guidance
(PDASHOSS01-196), so roll the new body forward in lockstep.

This migration calls ``seed_default_template(force=True)``. The seed
function is a no-op if the stored body already equals the assembled
fragments (e.g. on a freshly seeded instance), so re-running is safe.
"""

from __future__ import annotations

from django.db import migrations


def reseed_default_template(apps, schema_editor):
    # Lazy import — the migration framework's app-loading should not
    # pull in unrelated modules at definition time.
    from pi_dash.prompting.seed import seed_default_template

    seed_default_template(force=True)


def noop_reverse(apps, schema_editor):
    # No reverse — the prior body is not preserved.
    return


class Migration(migrations.Migration):
    dependencies = [
        ("prompting", "0006_reseed_relationships_section"),
    ]

    operations = [
        migrations.RunPython(reseed_default_template, reverse_code=noop_reverse),
    ]
