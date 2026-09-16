# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``Issue.complexity_score``.

An integer AI-reasoning-complexity rating for the task, used to route the
issue to an LLM of matching capability. ``0`` means "no score" (unrated) and
is the default; a rated issue takes an integer ``1..10``. The ``0..10`` bound
is enforced by model validators (surfaced by DRF on the write serializer).

No data migration: every existing issue takes the ``0`` default (unrated),
which is the intended starting state. How the score is derived is out of scope.
"""

from __future__ import annotations

import django.core.validators
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0158_test_cadence_fields"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="complexity_score",
            field=models.IntegerField(
                default=0,
                validators=[
                    django.core.validators.MinValueValidator(0),
                    django.core.validators.MaxValueValidator(10),
                ],
                verbose_name="AI Reasoning Complexity Score",
            ),
        ),
    ]
