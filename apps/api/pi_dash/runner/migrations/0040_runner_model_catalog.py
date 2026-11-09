# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner LLM model catalog (PDASHOSS01-364).

Instance-wide table backing the add-runner picker. One row per
(agent, model); ``reasoning_efforts`` holds the effort levels the
model offers (empty when the agent has no effort flag). The data
migration seeds today's frontend catalog verbatim so day one shows
no visible change; ``claude-opus-4-8`` is the preselected row.

Pruned from the autodetector's output: it also carried unrelated
drift on main (executor choices, index renames, RunMessageDedupe id,
constraint remove/re-add); those belong to their own migrations.
"""

import uuid

from django.db import migrations, models

# Today's catalog as defined in both frontends (apps/web_new
# catalog.ts and apps/web runner-models.ts). Codex rows were a
# 7 x 4 cross product; they are 7 rows with the full effort set here.
_SEED = [
    # claude-code (6) — opus 4.8 preselected
    ("claude-code", "claude-fable-5", "Fable 5", [], 0, False),
    ("claude-code", "claude-opus-5-5", "Opus 5.5", [], 1, False),
    ("claude-code", "claude-opus-5", "Opus 5", [], 2, False),
    ("claude-code", "claude-opus-4-8", "Opus 4.8", [], 3, True),
    ("claude-code", "claude-sonnet-4-6", "Sonnet 4.6", [], 4, False),
    ("claude-code", "claude-sonnet-4-6[1m]", "Sonnet 4.6 (1M context)", [], 5, False),
    # codex (7) — each offers all four effort levels
    ("codex", "gpt-6-astra", "GPT-6 Astra", ["low", "medium", "high", "xhigh"], 0, False),
    ("codex", "gpt-5.6-sol", "GPT-5.6 Sol", ["low", "medium", "high", "xhigh"], 1, False),
    ("codex", "gpt-5.6-terra", "GPT-5.6 Terra", ["low", "medium", "high", "xhigh"], 2, False),
    ("codex", "gpt-5.6-luna", "GPT-5.6 Luna", ["low", "medium", "high", "xhigh"], 3, False),
    ("codex", "gpt-5.5", "GPT-5.5", ["low", "medium", "high", "xhigh"], 4, False),
    ("codex", "gpt-5.4", "GPT-5.4", ["low", "medium", "high", "xhigh"], 5, False),
    ("codex", "gpt-5.4-mini", "GPT-5.4 Mini", ["low", "medium", "high", "xhigh"], 6, False),
    # cursor-agent (12) — tier baked into the slug, no effort flag
    ("cursor-agent", "auto", "Auto", [], 0, False),
    ("cursor-agent", "composer-2.5", "Composer 2.5", [], 1, False),
    ("cursor-agent", "gpt-5.5-high", "GPT-5.5 (High)", [], 2, False),
    ("cursor-agent", "gpt-5.5-extra-high", "GPT-5.5 (Extra High)", [], 3, False),
    ("cursor-agent", "gpt-5.4-medium", "GPT-5.4", [], 4, False),
    ("cursor-agent", "gpt-5.4-mini-medium", "GPT-5.4 Mini", [], 5, False),
    ("cursor-agent", "claude-opus-4-8-high", "Claude Opus 4.8", [], 6, False),
    ("cursor-agent", "claude-opus-4-8-thinking-high", "Claude Opus 4.8 (Thinking)", [], 7, False),
    ("cursor-agent", "claude-4.6-sonnet-medium", "Claude Sonnet 4.6", [], 8, False),
    ("cursor-agent", "claude-4.6-sonnet-medium-thinking", "Claude Sonnet 4.6 (Thinking)", [], 9, False),
    ("cursor-agent", "gemini-3.1-pro", "Gemini 3.1 Pro", [], 10, False),
    ("cursor-agent", "grok-4.3", "Grok 4.3", [], 11, False),
    # grok (2)
    ("grok", "grok-4.3", "Grok 4.3", [], 0, False),
    ("grok", "grok-code-fast-1", "Grok Code Fast 1", [], 1, False),
    # open-claw / muse-code: no curated rows (sentinel only in the picker)
]


def seed_catalog(apps, schema_editor):
    RunnerModelCatalog = apps.get_model("runner", "RunnerModelCatalog")
    for agent, model, label, efforts, sort_order, preselected in _SEED:
        RunnerModelCatalog.objects.update_or_create(
            agent=agent,
            model=model,
            defaults={
                "id": uuid.uuid4(),
                "label": label,
                "reasoning_efforts": efforts,
                "sort_order": sort_order,
                "is_enabled": True,
                "is_preselected": preselected,
            },
        )


def unseed_catalog(apps, schema_editor):
    # Only the rows this seed created, so a rollback does not wipe
    # rows an admin added afterwards.
    RunnerModelCatalog = apps.get_model("runner", "RunnerModelCatalog")
    for agent, model, _label, _efforts, _sort, _pre in _SEED:
        RunnerModelCatalog.objects.filter(agent=agent, model=model).delete()


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0039_issue_assigned_runner"),
    ]

    operations = [
        migrations.CreateModel(
            name="RunnerModelCatalog",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                (
                    "agent",
                    models.CharField(
                        choices=[
                            ("claude-code", "claude-code"),
                            ("codex", "codex"),
                            ("cursor-agent", "cursor-agent"),
                            ("open-claw", "open-claw"),
                            ("grok", "grok"),
                            ("muse-code", "muse-code"),
                        ],
                        db_index=True,
                        max_length=32,
                    ),
                ),
                ("model", models.CharField(max_length=128)),
                ("label", models.CharField(max_length=128)),
                ("reasoning_efforts", models.JSONField(blank=True, default=list)),
                ("sort_order", models.PositiveIntegerField(default=0)),
                ("is_enabled", models.BooleanField(default=True)),
                ("is_preselected", models.BooleanField(default=False)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
            ],
            options={
                "db_table": "runner_model_catalog",
                "ordering": ("agent", "sort_order", "created_at"),
            },
        ),
        migrations.AddIndex(
            model_name="runnermodelcatalog",
            index=models.Index(fields=["agent", "sort_order"], name="runner_model_agent_sort_idx"),
        ),
        migrations.AddIndex(
            model_name="runnermodelcatalog",
            index=models.Index(fields=["agent", "is_enabled"], name="runner_model_agent_enabled_idx"),
        ),
        migrations.AddConstraint(
            model_name="runnermodelcatalog",
            constraint=models.UniqueConstraint(fields=("agent", "model"), name="runner_model_unique_per_agent"),
        ),
        migrations.AddConstraint(
            model_name="runnermodelcatalog",
            constraint=models.UniqueConstraint(
                condition=models.Q(("is_preselected", True)),
                fields=("agent",),
                name="runner_model_one_preselected_per_agent",
            ),
        ),
        migrations.RunPython(seed_catalog, unseed_catalog),
    ]
