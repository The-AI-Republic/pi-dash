# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop the two dead worktree-pool columns.

The worktree pool is retired (PDASHOSS01-134 removed it from the daemon,
PDASHOSS01-137 retired the cloud-side half). Two columns added by
``0017_worktree_pooling`` outlived the retirement and have no live readers or
writers:

- ``Runner.free_worktrees`` — the per-runner capacity hint. No longer written
  or read; the matcher ranks eligible idle runners by heartbeat alone.
- ``AgentRun.queue_position`` — the display-only local-queue position. Every
  remaining write only cleared it to ``None``.

Dropping ``queue_position`` requires re-creating the
``agent_run_cloud_has_no_local_assignment`` check constraint without its
``queue_position__isnull=True`` term, so this migration pairs a
Remove/AddConstraint around the field drop.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0024_managed_runner_provisioning"),
    ]

    operations = [
        migrations.RemoveConstraint(
            model_name="agentrun",
            name="agent_run_cloud_has_no_local_assignment",
        ),
        migrations.RemoveField(
            model_name="runner",
            name="free_worktrees",
        ),
        migrations.RemoveField(
            model_name="agentrun",
            name="queue_position",
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.CheckConstraint(
                check=models.Q(("executor_kind__in", ["local_runner", "managed_runner"]))
                | models.Q(
                    ("executor_kind", "cloud_agent"),
                    ("runner__isnull", True),
                    ("pinned_runner__isnull", True),
                    ("owner__isnull", True),
                    ("assigned_at__isnull", True),
                ),
                name="agent_run_cloud_has_no_local_assignment",
            ),
        ),
    ]
