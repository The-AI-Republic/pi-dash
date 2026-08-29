# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two leaf migrations off 0152_git_generalization: the Cloud
Agent executor field on Project (0153_project_default_agent_executor) and
main's join-request / test-state chain (0153_workspace_join_request …
0158_test_cadence_fields).

The branches touch disjoint fields — one adds ``default_agent_executor``,
the other adds join requests, the In Test state group, and cadence fields —
so no ordering matters between them; this migration only rejoins the graph
into a single leaf so `migrate` runs."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0153_project_default_agent_executor"),
        ("db", "0158_test_cadence_fields"),
    ]

    operations = []
