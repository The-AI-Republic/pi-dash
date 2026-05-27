# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add GIN full-text-search index on ``Issue.name`` + ``description_stripped``.

Replaces the previous ``__icontains`` search path with stemming-aware
``to_tsvector('english', ...)`` matching backed by a GIN index. The
runtime query in ``pi_dash.search.issue`` and the issue branches of
``GlobalSearchEndpoint`` / ``SearchEndpoint`` must use an identical
``SearchVector('name', 'description_stripped', config='english')``
expression so the planner picks up this index.
"""

from __future__ import annotations

from django.contrib.postgres.indexes import GinIndex
from django.contrib.postgres.search import SearchVector
from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0141_issue_workpad"),
    ]

    operations = [
        migrations.AddIndex(
            model_name="issue",
            index=GinIndex(
                SearchVector("name", "description_stripped", config="english"),
                name="issues_fts_idx",
            ),
        ),
    ]
