# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two leaf migrations off 0023_one_active_includes_cancel_requested:
``0024_agentrun_agent_metadata`` (PDASHOSS01-100, the local agent session id)
and the ticking chain ``0024_managed_runner_provisioning`` →
``0025_agentrun_phase_kind``.

Both branches were cut from 0023 and neither was rebased onto the other, so
main carried two leaves and ``migrate`` refused to run at all:

    CommandError: Conflicting migrations detected; multiple leaf nodes in the
    migration graph: (0024_agentrun_agent_metadata, 0025_agentrun_phase_kind
    in runner).

The branches touch disjoint columns — ``AgentRun.agent_metadata`` on one side,
``DevMachine`` provisioning fields plus ``AgentRun.phase_kind`` on the other —
so no ordering matters between them. This migration only rejoins the graph
into a single leaf."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0024_agentrun_agent_metadata"),
        ("runner", "0025_agentrun_phase_kind"),
    ]

    operations = []
