# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two ``0156`` leaves that landed on main in parallel.

``0156_increase_in_progress_interval`` (PDASHOSS01-78) and
``0156_review_interval_8h`` (PDASHOSS01-70) both branched off ``0155``
and were merged independently, leaving the ``db`` app with two leaf
nodes — ``migrate`` refuses to run against a graph in that state
("Conflicting migrations detected; multiple leaf nodes in the migration
graph"). The two touch disjoint fields
(``Project.agent_default_interval_seconds`` vs
``Project.agent_review_default_interval_seconds``), so this is a pure
graph merge with no operations of its own.
"""

from __future__ import annotations

from django.db import migrations


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0156_increase_in_progress_interval"),
        ("db", "0156_review_interval_8h"),
    ]

    operations = []
