# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two 0182 heads (NEWPIDASH-305 AI Completed backfill and
PDASHOSS01-375 project settings) created when the 305 branch synced with
main. No schema change; joins both leaves into one head."""

from django.db import migrations


class Migration(migrations.Migration):

    dependencies = [
        ('db', '0182_ai_completed_default_state'),
        ('db', '0182_project_settings'),
    ]

    operations = [
    ]
