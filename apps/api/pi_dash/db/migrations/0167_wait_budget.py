# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The agent-called wait replaces the platform-driven blocker pause (PDASHOSS01-204).

Two column changes, both on the same idea: the platform stops deciding when
someone else's wait ends, and starts honouring an explicit request instead.

- Add ``IssueAgentTicker.waited`` — the ticks bought back by
  ``pidash issue wait``. It joins ``granted`` in the effective cap
  (pool + granted + waited), so a run that ends by waiting costs no net
  budget. Existing rows start at ``0``: nothing has waited yet.
- Drop ``Project.agent_wait_max_pause_seconds`` (added in 0165). It capped
  how long the scheduler would honour a ``Waiting on:`` marker parsed out of
  the agent's workpad. Nothing parses the workpad any more, so the policy has
  nothing left to govern.

0165 is not rewritten — it is merged and deployed. Reverse restores the
column at its old default so a downgrade validates against the 0166 model.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0166_drop_agent_retick_grant"),
    ]

    operations = [
        migrations.AddField(
            model_name="issueagentticker",
            name="waited",
            field=models.IntegerField(default=0),
        ),
        migrations.RemoveField(
            model_name="project",
            name="agent_wait_max_pause_seconds",
        ),
    ]
