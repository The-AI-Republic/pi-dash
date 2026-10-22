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

import django.db.models.deletion
from django.db import migrations, models


OLD_POOL_DEFAULT = 24
NEW_POOL_DEFAULT = 10

#: Pre-pool per-phase policy: state group → (ticker override column,
#: project default column). Only the *override* carried Re-tick grants
#: (``re_tick_ticker`` bumped it by one phase budget per press).
_PHASE_COLUMNS = {
    "started": ("max_ticks", "agent_default_max_ticks"),
    "review": ("review_max_ticks", "agent_review_default_max_ticks"),
    "test": ("test_max_ticks", "agent_test_default_max_ticks"),
}


def grant_from_override(override, project_default):
    """Runs a prior Re-tick added on top of the project default, or 0."""
    if override is None or override < 0:
        return 0
    return max(override - (project_default or 0), 0)


def fold_retick_grants_into_granted(apps, schema_editor):
    """Carry every prior Re-tick grant into the pool.

    Under the old model a grant was persisted by raising the issue's
    per-phase cap override above the project default for the phase the
    issue was in. Convert ``override − project default`` (when positive) on
    the issue's *current* phase into ``granted`` so the human's extra budget
    survives the switch to one pool. Overrides on other phases were only
    ever reachable by re-entering that phase; they are dropped with the
    columns.
    """
    IssueAgentTicker = apps.get_model("db", "IssueAgentTicker")
    for ticker in IssueAgentTicker.objects.select_related("issue__state", "issue__project").iterator():
        state = ticker.issue.state
        group = getattr(state, "group", None)
        columns = _PHASE_COLUMNS.get(group)
        if columns is None:
            continue
        grant = grant_from_override(
            getattr(ticker, columns[0], None),
            getattr(ticker.issue.project, columns[1], None),
        )
        if grant > 0:
            ticker.granted = grant
            ticker.save(update_fields=["granted"])


def move_projects_to_new_pool_default(apps, schema_editor):
    Project = apps.get_model("db", "Project")
    Project.objects.filter(agent_default_max_ticks=OLD_POOL_DEFAULT).update(
        agent_default_max_ticks=NEW_POOL_DEFAULT
    )


def stamp_pool_spent_on_rows_over_the_new_pool(apps, schema_editor):
    """An armed ticker whose ``used`` already meets the (smaller) pool must
    not keep advertising a live clock: the scanner would never admit it, so
    ``fire_tick`` would never stop it and the card would show a countdown
    forever. Stamp it ``pool_spent`` — *not* ``cap_hit``, which is the one
    reason that auto-Pauses an In Progress issue at its next run end — so the
    issue stays in the bucket with Re-tick offered."""
    IssueAgentTicker = apps.get_model("db", "IssueAgentTicker")
    for ticker in IssueAgentTicker.objects.filter(enabled=True).select_related("issue__project").iterator():
        pool = ticker.issue.project.agent_default_max_ticks
        if pool == -1:
            continue
        if ticker.used >= pool + ticker.granted:
            ticker.enabled = False
            ticker.disarm_reason = "pool_spent"
            ticker.save(update_fields=["enabled", "disarm_reason"])


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
        migrations.AddField(
            model_name="issueagentticker",
            name="pending_entry_actor",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="+",
                to="db.user",
            ),
        ),
        migrations.AddField(
            model_name="issueagentticker",
            name="pending_entry_trigger",
            field=models.CharField(blank=True, default="", max_length=24),
        ),
        migrations.AlterField(
            model_name="issueagentticker",
            name="disarm_reason",
            field=models.CharField(
                blank=True,
                choices=[
                    ("", "None"),
                    ("left_ticking_state", "Left Ticking State"),
                    ("cap_hit", "Cap Hit"),
                    ("pool_spent", "Pool Spent"),
                    ("terminal_signal", "Terminal Signal"),
                    ("user_disabled", "User Disabled"),
                ],
                default="",
                max_length=32,
            ),
        ),
        # Grants must be folded while the override columns still exist.
        migrations.RunPython(fold_retick_grants_into_granted, noop),
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
        # Only after the pool default is in place.
        migrations.RunPython(stamp_pool_spent_on_rows_over_the_new_pool, noop),
    ]
