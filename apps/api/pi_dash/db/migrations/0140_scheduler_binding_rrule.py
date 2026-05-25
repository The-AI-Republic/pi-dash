# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Migrate SchedulerBinding from cron to iCal-shaped recurrence.

See .ai_design/project_scheduler_calendar/decisions.md §1.

Order of operations:

1. Add ``Scheduler.color`` with default ``#3b82f6`` (deterministic backfill
   to a palette index happens in the same migration via RunPython).
2. Add ``SchedulerBinding.dtstart`` (nullable for now), ``tzid``,
   ``rrule``, ``rdates``, ``exdates``.
3. Backfill: for each binding, convert ``cron`` → ``rrule`` and set
   ``dtstart`` = ``created_at`` rounded forward to next valid firing.
4. Make ``dtstart`` non-null.
5. Drop the ``cron`` column.

Reverse direction is not supported — this is a one-way schema cleanup.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone as dt_timezone

from django.db import migrations, models

# Same 16-color palette referenced by the decisions doc (§6). Schedulers
# are assigned colors by index modulo 16 so the first 16 schedulers in a
# workspace get distinct colors.
_PALETTE = [
    "#3b82f6", "#6366f1", "#8b5cf6", "#a855f7",
    "#d946ef", "#ec4899", "#ef4444", "#f97316",
    "#eab308", "#84cc16", "#22c55e", "#10b981",
    "#14b8a6", "#06b6d4", "#0ea5e9", "#f59e0b",
]


def _convert_bindings(apps, schema_editor):
    """Backfill ``dtstart`` + ``rrule`` from the existing ``cron`` column.

    Imported from inside the function so the migration can run before
    the bgtasks module is importable (Django apps registry order).
    """
    from pi_dash.bgtasks._rrule import cron_to_rrule, CronConversionError, next_fire_from_rrule

    SchedulerBinding = apps.get_model("db", "SchedulerBinding")
    now = datetime.now(tz=dt_timezone.utc)

    failed: list[tuple[str, str, str]] = []  # (binding_id, cron, reason)

    for binding in SchedulerBinding.objects.all():
        cron = (binding.cron or "").strip()
        if not cron:
            # Should never happen — model required cron — but handle defensively.
            failed.append((str(binding.pk), "(empty)", "empty cron"))
            binding.dtstart = binding.created_at or now
            binding.rrule = ""
            binding.tzid = "UTC"
            binding.rdates = []
            binding.exdates = []
            binding.enabled = False
            binding.last_error = "migration: empty cron, binding disabled"
            binding.save(update_fields=[
                "dtstart", "rrule", "tzid", "rdates", "exdates",
                "enabled", "last_error", "updated_at",
            ])
            continue

        try:
            rrule_str = cron_to_rrule(cron)
        except CronConversionError as e:
            failed.append((str(binding.pk), cron, str(e)))
            # Preserve the binding row but disable it; operator must fix.
            binding.dtstart = binding.created_at or now
            binding.rrule = ""
            binding.tzid = "UTC"
            binding.rdates = []
            binding.exdates = []
            binding.enabled = False
            binding.last_error = f"migration: unconvertible cron {cron!r}: {e}"
            binding.save(update_fields=[
                "dtstart", "rrule", "tzid", "rdates", "exdates",
                "enabled", "last_error", "updated_at",
            ])
            continue

        # Anchor dtstart at "next valid firing after created_at" so the
        # series phase aligns with what cron would have produced.
        anchor = binding.created_at or now
        if anchor.tzinfo is None:
            anchor = anchor.replace(tzinfo=dt_timezone.utc)
        # Look one second before the anchor so the next-fire computation
        # returns the anchor itself if it's a valid firing.
        anchor_lookback = anchor - timedelta(seconds=1)
        dtstart = next_fire_from_rrule(
            dtstart=anchor_lookback,
            rrule_str=rrule_str,
            tzid="UTC",
            now=anchor_lookback,
        )
        if dtstart is None:
            # Converter produced a syntactically valid RRULE that
            # dateutil couldn't compute a next fire for. Same fallback as
            # the conversion-failure path.
            failed.append((str(binding.pk), cron, f"rrule {rrule_str!r} produced no next-fire"))
            binding.dtstart = anchor
            binding.rrule = ""
            binding.tzid = "UTC"
            binding.rdates = []
            binding.exdates = []
            binding.enabled = False
            binding.last_error = f"migration: rrule produced no next-fire: {rrule_str}"
            binding.save(update_fields=[
                "dtstart", "rrule", "tzid", "rdates", "exdates",
                "enabled", "last_error", "updated_at",
            ])
            continue

        binding.dtstart = dtstart
        binding.rrule = rrule_str
        binding.tzid = "UTC"
        binding.rdates = []
        binding.exdates = []
        binding.save(update_fields=[
            "dtstart", "rrule", "tzid", "rdates", "exdates", "updated_at"
        ])

    if failed:
        # Log via print; Django routes migration stdout through manage.py.
        print(
            f"\n[migration 0140] {len(failed)} binding(s) could not be converted "
            f"and have been disabled with last_error set:"
        )
        for bid, cron, reason in failed:
            print(f"  - {bid}: cron={cron!r} reason={reason}")


def _assign_scheduler_colors(apps, schema_editor):
    """Auto-assign colors to existing schedulers, per workspace."""
    Scheduler = apps.get_model("db", "Scheduler")
    # Group by workspace so each workspace's first 16 schedulers get
    # distinct colors regardless of cross-workspace ordering.
    by_workspace: dict = {}
    for scheduler in Scheduler.objects.all().order_by("workspace_id", "created_at"):
        idx = by_workspace.get(scheduler.workspace_id, 0)
        scheduler.color = _PALETTE[idx % len(_PALETTE)]
        scheduler.save(update_fields=["color", "updated_at"])
        by_workspace[scheduler.workspace_id] = idx + 1


def _noop_reverse(apps, schema_editor):
    """Migration is one-way; reverse is a no-op so squash/zero-rollouts work."""


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0138_merge_default_project_and_github_sync"),
    ]

    operations = [
        # --- Scheduler.color ---
        migrations.AddField(
            model_name="scheduler",
            name="color",
            field=models.CharField(default="#3b82f6", max_length=7),
        ),
        migrations.RunPython(_assign_scheduler_colors, _noop_reverse),

        # --- SchedulerBinding new fields (nullable / defaults so existing rows stay valid) ---
        migrations.AddField(
            model_name="schedulerbinding",
            name="dtstart",
            field=models.DateTimeField(null=True),
        ),
        migrations.AddField(
            model_name="schedulerbinding",
            name="tzid",
            field=models.CharField(default="UTC", max_length=64),
        ),
        migrations.AddField(
            model_name="schedulerbinding",
            name="rrule",
            field=models.TextField(blank=True, default=""),
        ),
        migrations.AddField(
            model_name="schedulerbinding",
            name="rdates",
            field=models.JSONField(blank=True, default=list),
        ),
        migrations.AddField(
            model_name="schedulerbinding",
            name="exdates",
            field=models.JSONField(blank=True, default=list),
        ),

        # --- Backfill from cron ---
        migrations.RunPython(_convert_bindings, _noop_reverse),

        # --- Tighten dtstart to NOT NULL now that it's populated ---
        migrations.AlterField(
            model_name="schedulerbinding",
            name="dtstart",
            field=models.DateTimeField(),
        ),

        # --- Drop the legacy cron column ---
        migrations.RemoveField(
            model_name="schedulerbinding",
            name="cron",
        ),
    ]
