# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Refresh the global default PromptTemplate after the workpad rewrite.

``seed_default_template`` only fires with ``force=False`` from the
``post_migrate`` signal, so an existing instance keeps serving the
previously-seeded body even after fragment files change on disk. That
behavior is intentional for normal edits (operators decide when to roll
forward), but the workpad-on-Issue migration overhauls how agents
read/write their cross-run state — leaving the old body live would mean
agents keep calling ``pidash comment list | grep '## Agent Workpad'``
instead of ``pidash workpad get/update`` on existing deployments.

This migration calls ``seed_default_template(force=True)`` so the next
``manage.py migrate`` rolls the new body forward in lockstep with the
``Issue.workpad`` schema change. The seed function is a no-op if the
stored body already equals the assembled fragments (e.g. on a freshly
seeded instance), so re-running is safe.
"""

from __future__ import annotations

from django.db import migrations


def reseed_default_template(apps, schema_editor):
    # Lazy import — the migration framework's app-loading should not
    # pull in unrelated modules at definition time.
    from pi_dash.prompting.seed import seed_default_template

    seed_default_template(force=True)


def noop_reverse(apps, schema_editor):
    # No reverse — the prior body is not preserved. Rolling back the
    # ``Issue.workpad`` schema change is the explicit way to revert.
    return


class Migration(migrations.Migration):
    dependencies = [
        ("prompting", "0002_review_template"),
        # Tie this re-seed to the workpad field add so the new body
        # only goes live on an instance that has the field available.
        ("db", "0141_issue_workpad"),
    ]

    operations = [
        migrations.RunPython(reseed_default_template, reverse_code=noop_reverse),
    ]
