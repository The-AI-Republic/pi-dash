# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""One clock per issue, one budget pool.

``.ai_design/ticking_relevance/design.md`` §9. The ticker stops being torn
down and rebuilt on every stage change, and the three per-stage budgets
(24 / 4 / 3) collapse into one pool of 10 per issue spent in any stage.

``IssueAgentTicker``:

- ``tick_count`` → ``used`` (rename; values carry over).
- ``granted`` (Re-tick additions), ``pending_entry`` /
  ``pending_entry_free`` (the entry-run queue, design §4.5) — new.
- the per-issue cap and interval overrides (``max_ticks``,
  ``interval_seconds``, ``review_*``, ``test_*``) — dropped. Budget policy
  lives on the project; only consumption lives on the issue.

``Project``:

- ``agent_default_max_ticks`` is now the pool; default 24 → 10. Rows still
  sitting on the old default are moved to the new one so existing projects
  get the intended ceiling; explicitly tuned values are left alone.
- ``agent_retick_grant`` — new (3).
- ``agent_review_default_max_ticks`` / ``agent_test_default_max_ticks`` —
  dropped.
"""

from django.db import migrations, models


OLD_POOL_DEFAULT = 24
NEW_POOL_DEFAULT = 10


def move_projects_to_new_pool_default(apps, schema_editor):
    Project = apps.get_model("db", "Project")
    Project.objects.filter(agent_default_max_ticks=OLD_POOL_DEFAULT).update(
        agent_default_max_ticks=NEW_POOL_DEFAULT
    )


def noop(apps, schema_editor):
    pass


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0162_merge_complexity_and_profile_settings"),
        ("runner", "0024_managed_runner_provisioning"),
    ]

    operations = [
        # --- ticker: rename the counter, add the pool/queue fields -------
        migrations.RenameField(
            model_name="issueagentticker",
            old_name="tick_count",
            new_name="used",
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="granted",
            field=models.IntegerField(default=0),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="pending_entry",
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="pending_entry_free",
            field=models.BooleanField(default=False),
        ),
        # --- ticker: drop the per-issue policy overrides -----------------
        migrations.RemoveField(model_name="issueagentticker", name="interval_seconds"),
        migrations.RemoveField(model_name="issueagentticker", name="max_ticks"),
        migrations.RemoveField(model_name="issueagentticker", name="review_interval_seconds"),
        migrations.RemoveField(model_name="issueagentticker", name="review_max_ticks"),
        migrations.RemoveField(model_name="issueagentticker", name="test_interval_seconds"),
        migrations.RemoveField(model_name="issueagentticker", name="test_max_ticks"),
        # --- project: pool + grant, drop the per-stage caps --------------
        migrations.AlterField(
            model_name="project",
            name="agent_default_max_ticks",
            field=models.IntegerField(default=NEW_POOL_DEFAULT),
        ),
        migrations.RunPython(move_projects_to_new_pool_default, noop),
        migrations.AddField(
            model_name="project",
            name="agent_retick_grant",
            field=models.IntegerField(default=3),
        ),
        migrations.RemoveField(model_name="project", name="agent_review_default_max_ticks"),
        migrations.RemoveField(model_name="project", name="agent_test_default_max_ticks"),
    ]
