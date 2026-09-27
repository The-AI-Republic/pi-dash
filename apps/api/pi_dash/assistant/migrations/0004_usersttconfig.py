# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``UserSTTConfig`` — per-user BYO speech-to-text (dictation) config.

New table only; nothing existing is touched. Mirrors ``UserLLMConfig`` for the
OpenAI-compatible ``/v1/audio/transcriptions`` endpoint, minus the provider
selector (dictation has a single OpenAI-compatible provider). The API never
exposes the stored key, only ``has_api_key``.
"""

from django.conf import settings
from django.db import migrations, models
import django.db.models.deletion


class Migration(migrations.Migration):

    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("assistant", "0003_assistantmcpserver"),
    ]

    operations = [
        migrations.CreateModel(
            name="UserSTTConfig",
            fields=[
                (
                    "id",
                    models.BigAutoField(
                        auto_created=True, primary_key=True, serialize=False, verbose_name="ID"
                    ),
                ),
                ("base_url", models.URLField(blank=True, default="", max_length=500)),
                ("model_name", models.CharField(blank=True, default="", max_length=255)),
                ("api_key_encrypted", models.BinaryField(blank=True, null=True)),
                ("last_verified_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "user",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="assistant_stt_config",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "assistant_user_stt_config",
            },
        ),
    ]
