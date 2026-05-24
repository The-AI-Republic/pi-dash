# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Merge the two 0137 leaves: github-sync unique drop + default-project constraint."""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0137_drop_github_repository_sync_repository_unique"),
        ("db", "0137_project_unique_default_per_workspace"),
    ]

    operations = []
