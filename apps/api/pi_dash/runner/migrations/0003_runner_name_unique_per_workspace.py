# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Add ``UNIQUE(workspace_id, name)`` on ``runner``.

Part of PR 3 of the runner-install-UX redesign. Makes ``runner.name`` the
per-workspace human handle the CLI can address, while ``runner.id`` (UUID)
stays the wire identity.

See .ai_design/runner_install_ux/cli-restructure-and-install-flow.md.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0002_agentrun_parent_run_blocked_status"),
    ]

    operations = [
        migrations.AddConstraint(
            model_name="runner",
            constraint=models.UniqueConstraint(
                fields=["workspace", "name"],
                name="runner_unique_name_per_workspace",
            ),
        ),
    ]
