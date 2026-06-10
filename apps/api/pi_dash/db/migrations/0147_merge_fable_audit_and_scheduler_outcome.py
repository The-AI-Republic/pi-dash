# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two leaf migrations off 0144_issuecomment_speaker_metadata:
fable security-audit seed (#216) and schedulerbinding outcome_mode (#214)."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0145_seed_fable_security_audit"),
        ("db", "0146_schedulerbinding_outcome_mode"),
    ]

    operations = []
