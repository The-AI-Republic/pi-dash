# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two leaf migrations off 0018_agentrun_trigger_manifest: the
Cloud Agent execution fields (0019_cloud_agent_execution) and main's
machine-session / cancel-requested chain (0019_machine_session …
0021_agentrun_cancel_requested).

The branches touch disjoint columns — one adds executor/terminal-effect
fields and the tool-call table, the other adds machine sessions, runner
dev metadata, and the ``cancel_requested`` status choice — so no ordering
matters between them; this migration only rejoins the graph into a single
leaf so `migrate` runs."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0019_cloud_agent_execution"),
        ("runner", "0021_agentrun_cancel_requested"),
    ]

    operations = []
