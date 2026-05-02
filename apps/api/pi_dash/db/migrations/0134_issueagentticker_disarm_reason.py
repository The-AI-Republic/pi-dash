# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PR A / M0 — terminal-disarm safety field on IssueAgentTicker.

Adds ``disarm_reason`` (TextChoices, blank default) so
``maybe_apply_deferred_pause`` can auto-Pause only on cap-hit
disarms, never on terminal-signal disarms. Existing rows backfill
to the empty-string default — equivalent to ``NONE``, which is the
correct value while a ticker is armed.

See ``.ai_design/create_review_state/design.md`` §6.3 / §8.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0133_seed_builtin_schedulers"),
    ]

    operations = [
        migrations.AddField(
            model_name="issueagentticker",
            name="disarm_reason",
            field=models.CharField(
                blank=True,
                choices=[
                    ("", "None"),
                    ("left_ticking_state", "Left Ticking State"),
                    ("cap_hit", "Cap Hit"),
                    ("terminal_signal", "Terminal Signal"),
                    ("user_disabled", "User Disabled"),
                ],
                default="",
                max_length=32,
            ),
        ),
    ]
