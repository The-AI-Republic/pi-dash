# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add GIN full-text-search index on ``IssueComment.comment_stripped``.

Lets pi_dash.search.issue match issues by their comment text in addition
to title/description. Agents searching for historical context need this:
the *resolution* of an issue typically lives in a comment, not in the
original description.

Built concurrently (see ``0142`` for the same rationale — avoids the
write-blocking lock on the ``issue_comments`` table during deploy).
"""

from __future__ import annotations

from django.contrib.postgres.indexes import GinIndex
from django.contrib.postgres.operations import AddIndexConcurrently
from django.contrib.postgres.search import SearchVector
from django.db import migrations


class Migration(migrations.Migration):
    atomic = False

    dependencies = [
        ("db", "0142_issue_fts_idx"),
    ]

    operations = [
        AddIndexConcurrently(
            model_name="issuecomment",
            index=GinIndex(
                SearchVector("comment_stripped", config="english"),
                name="issue_comments_fts_idx",
            ),
        ),
    ]
