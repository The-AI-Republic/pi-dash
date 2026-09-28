# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop the inherited cover-image feature (PDASHOSS01-237).

Projects and users each carried two cover-image columns from upstream: a
free-text URL (``cover_image``, historically an Unsplash link) and a FK to an
uploaded ``FileAsset`` (``cover_image_asset``). Nothing in Pi Dash renders a
cover any more and the Unsplash picker that fed the URL column is gone, so
all four columns go.

The ``FileAsset`` rows that the FKs pointed at are left in place: they are
ordinary soft-deletable assets and the shared upload machinery still owns
them. Reverse restores the columns (nullable, empty) so a downgrade validates
against the 0167 model.
"""

from django.db import migrations


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0167_wait_budget"),
    ]

    operations = [
        migrations.RemoveField(
            model_name="project",
            name="cover_image",
        ),
        migrations.RemoveField(
            model_name="project",
            name="cover_image_asset",
        ),
        migrations.RemoveField(
            model_name="user",
            name="cover_image",
        ),
        migrations.RemoveField(
            model_name="user",
            name="cover_image_asset",
        ),
    ]
