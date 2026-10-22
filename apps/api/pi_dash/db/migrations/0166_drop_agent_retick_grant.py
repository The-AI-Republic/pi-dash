# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop ``Project.agent_retick_grant`` — the Re-tick grant is the pool.

``.ai_design/ticking_relevance/design.md`` §5.5. A Re-tick used to add a
fixed ``agent_retick_grant`` (default 3) to the issue's budget; it now adds a
fresh ``agent_default_max_ticks`` pool instead, so the separate grant column
is redundant. Existing ``IssueAgentTicker.granted`` values carry over
unchanged — this only removes the (unused-by-consumption) project column.

Reverse re-adds the column at its old default so a downgrade still validates
against the pre-0166 model.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0165_project_agent_wait_max_pause"),
    ]

    operations = [
        migrations.RemoveField(model_name="project", name="agent_retick_grant"),
    ]
