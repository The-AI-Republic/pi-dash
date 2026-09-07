# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AssistantMCPServer`` — user-configured MCP tool servers.

New table only; nothing existing is touched. Each enabled row becomes one
pydantic-ai toolset for the duration of an assistant run. The unique
(user, name) constraint keeps derived tool prefixes unambiguous.
"""

from django.conf import settings
from django.db import migrations, models
import django.db.models.deletion
import uuid


class Migration(migrations.Migration):

    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("assistant", "0002_assistantthread_kind"),
    ]

    operations = [
        migrations.CreateModel(
            name="AssistantMCPServer",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        default=uuid.uuid4, editable=False, primary_key=True, serialize=False
                    ),
                ),
                ("name", models.CharField(max_length=80)),
                ("url", models.URLField(max_length=500)),
                ("auth_header_encrypted", models.BinaryField(blank=True, null=True)),
                ("is_enabled", models.BooleanField(default=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="assistant_mcp_servers",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "assistant_mcp_server",
                "ordering": ("created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="assistantmcpserver",
            constraint=models.UniqueConstraint(
                fields=("user", "name"), name="assistant_mcp_server_user_name_uniq"
            ),
        ),
    ]
