# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the three leaf migrations off 0155_reduce_review_max_ticks:
In Progress interval 12 h (PDASHOSS01-78), In Review interval 8 h
(PDASHOSS01-70), and issuecomment labels (OPENHUB-45).

The three touch disjoint fields — two Project interval defaults and one
IssueComment field — so no ordering matters between them; this migration
only rejoins the graph into a single leaf so `migrate` runs."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0156_increase_in_progress_interval"),
        ("db", "0156_review_interval_8h"),
        ("db", "0156_issuecomment_labels"),
    ]

    operations = []
