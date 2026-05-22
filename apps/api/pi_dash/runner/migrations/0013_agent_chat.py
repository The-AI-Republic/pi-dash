# Generated manually for runner direct chat.

import django.db.models.deletion
import uuid
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0012_runner_live_state"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="AgentChatSession",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                (
                    "status",
                    models.CharField(
                        choices=[("open", "Open"), ("closed", "Closed"), ("failed", "Failed")],
                        db_index=True,
                        default="open",
                        max_length=24,
                    ),
                ),
                ("agent_kind", models.CharField(blank=True, default="", max_length=24)),
                ("local_thread_id", models.CharField(blank=True, default="", max_length=128)),
                ("local_session_id", models.CharField(blank=True, default="", max_length=128)),
                ("cwd", models.TextField(blank=True, default="")),
                ("model", models.CharField(blank=True, default="", max_length=128)),
                ("active_turn_id", models.CharField(blank=True, default="", max_length=128)),
                ("active_message_id", models.UUIDField(blank=True, null=True)),
                ("close_requested", models.BooleanField(default=False)),
                ("last_message_at", models.DateTimeField(blank=True, null=True)),
                ("closed_at", models.DateTimeField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.PROTECT,
                        related_name="agent_chat_sessions",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "pod",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.PROTECT, related_name="agent_chat_sessions", to="runner.pod"
                    ),
                ),
                (
                    "runner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE, related_name="chat_sessions", to="runner.runner"
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="agent_chat_sessions",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "agent_chat_session",
                "ordering": ("-last_message_at", "-created_at"),
            },
        ),
        migrations.CreateModel(
            name="AgentChatMessage",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                (
                    "role",
                    models.CharField(
                        choices=[("user", "User"), ("assistant", "Assistant"), ("tool", "Tool"), ("system", "System")],
                        db_index=True,
                        max_length=16,
                    ),
                ),
                ("content", models.TextField(blank=True, default="")),
                ("content_parts", models.JSONField(blank=True, default=list)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("queued", "Queued"),
                            ("sent", "Sent"),
                            ("streaming", "Streaming"),
                            ("completed", "Completed"),
                            ("failed", "Failed"),
                            ("cancelled", "Cancelled"),
                        ],
                        db_index=True,
                        default="completed",
                        max_length=24,
                    ),
                ),
                ("local_item_id", models.CharField(blank=True, default="", max_length=128)),
                ("local_turn_id", models.CharField(blank=True, default="", max_length=128)),
                ("seq", models.PositiveIntegerField()),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("completed_at", models.DateTimeField(blank=True, null=True)),
                (
                    "session",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="messages",
                        to="runner.agentchatsession",
                    ),
                ),
            ],
            options={
                "db_table": "agent_chat_message",
                "ordering": ("session", "seq"),
            },
        ),
        migrations.CreateModel(
            name="AgentChatEvent",
            fields=[
                ("id", models.BigAutoField(primary_key=True, serialize=False)),
                ("seq", models.PositiveIntegerField()),
                ("source_key", models.CharField(blank=True, default="", max_length=160)),
                ("kind", models.CharField(max_length=64)),
                ("payload", models.JSONField(blank=True, default=dict)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "message",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="events",
                        to="runner.agentchatmessage",
                    ),
                ),
                (
                    "session",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE, related_name="events", to="runner.agentchatsession"
                    ),
                ),
            ],
            options={
                "db_table": "agent_chat_event",
                "ordering": ("session", "seq"),
            },
        ),
        migrations.CreateModel(
            name="AgentChatApprovalRequest",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("local_approval_id", models.CharField(max_length=160)),
                (
                    "kind",
                    models.CharField(
                        choices=[
                            ("command_execution", "Command Execution"),
                            ("file_change", "File Change"),
                            ("network_access", "Network Access"),
                            ("other", "Other"),
                        ],
                        max_length=24,
                    ),
                ),
                ("payload", models.JSONField(blank=True, default=dict)),
                ("reason", models.TextField(blank=True, default="")),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("pending", "Pending"),
                            ("accepted", "Accepted"),
                            ("declined", "Declined"),
                            ("expired", "Expired"),
                        ],
                        db_index=True,
                        default="pending",
                        max_length=16,
                    ),
                ),
                ("decision_source", models.CharField(blank=True, default="", max_length=16)),
                ("requested_at", models.DateTimeField(auto_now_add=True)),
                ("expires_at", models.DateTimeField(blank=True, null=True)),
                ("decided_at", models.DateTimeField(blank=True, null=True)),
                (
                    "decided_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="runner_chat_approvals_decided",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "session",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="approvals",
                        to="runner.agentchatsession",
                    ),
                ),
            ],
            options={
                "db_table": "agent_chat_approval",
                "ordering": ("-requested_at",),
            },
        ),
        migrations.CreateModel(
            name="ChatMessageDedupe",
            fields=[
                ("id", models.BigAutoField(auto_created=True, primary_key=True, serialize=False, verbose_name="ID")),
                ("message_id", models.CharField(max_length=128)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "session",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="message_dedupes",
                        to="runner.agentchatsession",
                    ),
                ),
            ],
            options={
                "db_table": "chat_message_dedupe",
            },
        ),
        migrations.AddIndex(
            model_name="agentchatsession",
            index=models.Index(fields=["workspace", "runner", "status"], name="ac_sess_ws_run_stat_idx"),
        ),
        migrations.AddIndex(
            model_name="agentchatsession",
            index=models.Index(fields=["created_by", "runner", "status"], name="ac_sess_user_run_stat_idx"),
        ),
        migrations.AddIndex(
            model_name="agentchatsession", index=models.Index(fields=["runner", "status"], name="ac_sess_run_stat_idx")
        ),
        migrations.AddIndex(
            model_name="agentchatsession", index=models.Index(fields=["last_message_at"], name="ac_sess_last_msg_idx")
        ),
        migrations.AddIndex(
            model_name="agentchatmessage",
            index=models.Index(fields=["session", "created_at"], name="ac_msg_sess_created_idx"),
        ),
        migrations.AddIndex(
            model_name="agentchatmessage",
            index=models.Index(fields=["session", "local_turn_id"], name="ac_msg_sess_turn_idx"),
        ),
        migrations.AddIndex(
            model_name="agentchatmessage",
            index=models.Index(fields=["session", "local_item_id"], name="ac_msg_sess_item_idx"),
        ),
        migrations.AddConstraint(
            model_name="agentchatmessage",
            constraint=models.UniqueConstraint(fields=("session", "seq"), name="ac_msg_sess_seq_uniq"),
        ),
        migrations.AddIndex(
            model_name="agentchatevent",
            index=models.Index(fields=["session", "created_at"], name="ac_evt_sess_created_idx"),
        ),
        migrations.AddConstraint(
            model_name="agentchatevent",
            constraint=models.UniqueConstraint(fields=("session", "seq"), name="ac_evt_sess_seq_uniq"),
        ),
        migrations.AddConstraint(
            model_name="agentchatevent",
            constraint=models.UniqueConstraint(
                condition=~models.Q(("source_key", "")), fields=("session", "source_key"), name="ac_evt_source_key_uniq"
            ),
        ),
        migrations.AddIndex(
            model_name="agentchatapprovalrequest",
            index=models.Index(fields=["session", "status"], name="ac_appr_sess_stat_idx"),
        ),
        migrations.AddConstraint(
            model_name="agentchatapprovalrequest",
            constraint=models.UniqueConstraint(fields=("session", "local_approval_id"), name="ac_appr_local_uniq"),
        ),
        migrations.AddIndex(
            model_name="chatmessagededupe", index=models.Index(fields=["created_at"], name="chat_dedupe_created_idx")
        ),
        migrations.AddConstraint(
            model_name="chatmessagededupe",
            constraint=models.UniqueConstraint(fields=("session", "message_id"), name="chat_dedupe_unique"),
        ),
    ]
