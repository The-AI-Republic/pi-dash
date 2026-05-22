# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge latest main's CLI device-code migration with review-state work."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0135_clidevicecode"),
        ("db", "0135_review_state_and_cadence_split"),
    ]

    operations = []
