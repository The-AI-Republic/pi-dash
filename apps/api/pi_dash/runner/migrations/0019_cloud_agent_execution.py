import uuid

from django.db import migrations, models
import django.db.models.deletion


TERMINAL = ["completed", "failed", "cancelled", "blocked", "refused"]
ACTIVE = ["queued", "assigned", "waiting_for_worktree", "running", "awaiting_approval", "awaiting_reauth"]


def prepare_existing_rows(apps, schema_editor):
    AgentRun = apps.get_model("runner", "AgentRun")
    duplicates = (
        AgentRun.objects.filter(work_item__isnull=False, status__in=ACTIVE)
        .values("work_item_id")
        .annotate(count=models.Count("id"))
        .filter(count__gt=1)
    )
    duplicate_ids = []
    for row in duplicates:
        duplicate_ids.extend(
            str(value)
            for value in AgentRun.objects.filter(work_item_id=row["work_item_id"], status__in=ACTIVE).values_list(
                "id", flat=True
            )
        )
    if duplicate_ids:
        raise RuntimeError(
            "Cannot add one-active-run constraint; conflicting AgentRun IDs: " + ", ".join(duplicate_ids)
        )
    AgentRun.objects.filter(status__in=TERMINAL).update(
        terminal_hooks_applied_at=models.functions.Coalesce("ended_at", "created_at"),
        terminal_capacity_released_at=models.functions.Coalesce("ended_at", "created_at"),
    )


class Migration(migrations.Migration):
    dependencies = [("db", "0153_project_default_agent_executor"), ("runner", "0018_agentrun_trigger_manifest")]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="executor_kind",
            field=models.CharField(
                choices=[("local_runner", "Local Runner"), ("cloud_agent", "Pi Dash Cloud Agent")],
                db_index=True,
                default="local_runner",
                max_length=24,
            ),
        ),
        migrations.AddField(
            model_name="agentrun", name="dispatch_attempts", field=models.PositiveIntegerField(default=0)
        ),
        migrations.AddField(
            model_name="agentrun", name="cancel_requested_at", field=models.DateTimeField(blank=True, null=True)
        ),
        migrations.AddField(
            model_name="agentrun", name="cancel_reason", field=models.CharField(blank=True, default="", max_length=512)
        ),
        migrations.AddField(
            model_name="agentrun",
            name="error_code",
            field=models.CharField(blank=True, db_index=True, default="", max_length=64),
        ),
        migrations.AddField(model_name="agentrun", name="tool_plan", field=models.JSONField(blank=True, default=dict)),
        migrations.AddField(
            model_name="agentrun", name="terminal_hooks_applied_at", field=models.DateTimeField(blank=True, null=True)
        ),
        migrations.AddField(
            model_name="agentrun",
            name="terminal_capacity_released_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.CreateModel(
            name="AgentRunToolCall",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("tool_call_id", models.CharField(max_length=255)),
                ("source", models.CharField(max_length=16)),
                ("server_key", models.CharField(blank=True, default="", max_length=64)),
                ("tool_name", models.CharField(max_length=255)),
                ("risk", models.CharField(max_length=16)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("prepared", "Prepared"),
                            ("submitted", "Submitted"),
                            ("succeeded", "Succeeded"),
                            ("failed", "Failed"),
                            ("unknown", "Outcome unknown"),
                            ("denied", "Denied"),
                        ],
                        default="prepared",
                        max_length=16,
                    ),
                ),
                ("request_fingerprint", models.CharField(max_length=64)),
                ("result_fingerprint", models.CharField(blank=True, default="", max_length=64)),
                ("idempotency_key_hash", models.CharField(blank=True, default="", max_length=64)),
                ("external_operation_id", models.CharField(blank=True, default="", max_length=255)),
                ("safe_replay_result", models.JSONField(blank=True, null=True)),
                ("error_code", models.CharField(blank=True, default="", max_length=64)),
                ("prepared_at", models.DateTimeField(auto_now_add=True)),
                ("submitted_at", models.DateTimeField(blank=True, null=True)),
                ("completed_at", models.DateTimeField(blank=True, null=True)),
                (
                    "agent_run",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE, related_name="tool_calls", to="runner.agentrun"
                    ),
                ),
            ],
            options={"db_table": "agent_run_tool_call"},
        ),
        migrations.RunPython(prepare_existing_rows, migrations.RunPython.noop),
        migrations.AddConstraint(
            model_name="agentruntoolcall",
            constraint=models.UniqueConstraint(fields=("agent_run", "tool_call_id"), name="agent_run_tool_call_unique"),
        ),
        migrations.AddIndex(
            model_name="agentruntoolcall",
            index=models.Index(fields=["agent_run", "status"], name="agent_run_tool_status_idx"),
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.CheckConstraint(
                check=models.Q(
                    ("executor_kind", "local_runner"),
                    models.Q(
                        ("assigned_at__isnull", True),
                        ("executor_kind", "cloud_agent"),
                        ("owner__isnull", True),
                        ("pinned_runner__isnull", True),
                        ("queue_position__isnull", True),
                        ("runner__isnull", True),
                    ),
                    _connector="OR",
                ),
                name="agent_run_cloud_has_no_local_assignment",
            ),
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.UniqueConstraint(
                condition=models.Q(("work_item__isnull", False), ("status__in", ACTIVE)),
                fields=("work_item",),
                name="agent_run_one_active_per_work_item",
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["executor_kind", "status", "lease_expires_at", "created_at"],
                name="agent_run_cloud_dispatch_idx",
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(fields=["executor_kind", "status", "started_at"], name="agent_run_cloud_stale_idx"),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["status", "terminal_hooks_applied_at", "ended_at"], name="agent_run_term_hooks_idx"
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["status", "terminal_capacity_released_at", "ended_at"], name="agent_run_term_capacity_idx"
            ),
        ),
    ]
