# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dry-run the cron → RRULE migration for every SchedulerBinding.

Per ``.ai_design/project_scheduler_calendar/decisions.md`` §1, this
command should be run against the live DB *before* migration 0140 is
deployed. It prints, per binding:

- the old cron string
- the cron's next firing per ``croniter``
- the converted RRULE string (or the conversion error)
- the RRULE's next firing per ``dateutil.rrule``
- a ``MATCH`` / ``MISMATCH`` / ``FAIL`` verdict

Output is plain text, suitable for pasting into the PR1 review thread.

Usage:

    python manage.py dry_run_scheduler_migration
    python manage.py dry_run_scheduler_migration --workspace acme
    python manage.py dry_run_scheduler_migration --json   # machine-readable

The command is read-only — it never writes to the DB. Safe to run in prod.

After migration 0140 lands, the ``cron`` column no longer exists and this
command is a no-op (it prints a notice and exits 0). The file stays in
the tree as historical context for the migration choice.
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timedelta, timezone as dt_timezone
from typing import Optional

from django.core.management.base import BaseCommand
from django.db import connection

from pi_dash.bgtasks._rrule import (
    CronConversionError,
    cron_to_rrule,
    next_fire_from_rrule,
)
from pi_dash.db.models.scheduler import SchedulerBinding


def _next_fire_from_cron_legacy(cron_expr: str, *, now: datetime) -> Optional[datetime]:
    """Compute next-fire via croniter, for the comparison column.

    Imported lazily so the command still runs after croniter is removed
    from requirements/base.txt — we just skip the comparison in that case.
    """
    try:
        from croniter import croniter, CroniterBadCronError
    except ImportError:
        return None

    try:
        itr = croniter(cron_expr, now)
        nxt = itr.get_next(datetime)
        if nxt.tzinfo is None:
            nxt = nxt.replace(tzinfo=dt_timezone.utc)
        return nxt
    except (CroniterBadCronError, ValueError):
        return None


def _has_cron_column() -> bool:
    """Pre-migration: cron column exists. Post-migration: gone."""
    with connection.cursor() as cur:
        cur.execute(
            """
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'scheduler_bindings' AND column_name = 'cron'
            """
        )
        return cur.fetchone() is not None


class Command(BaseCommand):
    help = "Dry-run the cron→RRULE migration. Read-only."

    def add_arguments(self, parser):
        parser.add_argument(
            "--workspace",
            help="Limit to bindings whose workspace has this slug",
            default=None,
        )
        parser.add_argument(
            "--json",
            action="store_true",
            help="Emit machine-readable JSON instead of human-readable rows",
        )

    def handle(self, *args, **options):
        if not _has_cron_column():
            self.stdout.write(
                "scheduler_bindings.cron column no longer exists — migration 0140 has already run."
            )
            return

        workspace_slug: Optional[str] = options.get("workspace")
        as_json: bool = bool(options.get("json"))

        qs = SchedulerBinding.objects.all()
        if workspace_slug:
            qs = qs.filter(workspace__slug=workspace_slug)
        # We need to read .cron through raw SQL since the model no longer
        # has the field after migration 0140 lands — but pre-migration, the
        # column does exist. Use .values() to fetch what we need without
        # depending on the Python field.
        rows = list(
            qs.values("id", "workspace_id", "workspace__slug", "created_at", "cron", "enabled")
        )

        now = datetime.now(tz=dt_timezone.utc)
        results = []
        match = mismatch = fail = 0

        for row in rows:
            bid = str(row["id"])
            cron = (row.get("cron") or "").strip()
            workspace = row.get("workspace__slug", "")
            created_at: datetime = row["created_at"]
            if created_at.tzinfo is None:
                created_at = created_at.replace(tzinfo=dt_timezone.utc)

            entry = {
                "binding_id": bid,
                "workspace": workspace,
                "enabled": row["enabled"],
                "cron": cron,
                "cron_next_fire": None,
                "rrule": None,
                "rrule_next_fire": None,
                "verdict": "FAIL",
                "reason": None,
            }

            cron_next = _next_fire_from_cron_legacy(cron, now=now)
            entry["cron_next_fire"] = cron_next.isoformat() if cron_next else None

            try:
                rrule_str = cron_to_rrule(cron)
                entry["rrule"] = rrule_str
            except CronConversionError as e:
                entry["reason"] = f"conversion error: {e}"
                results.append(entry)
                fail += 1
                continue

            # Anchor dtstart at next valid firing after created_at, mirroring
            # the migration's logic.
            anchor_lookback = created_at - timedelta(seconds=1)
            dtstart = next_fire_from_rrule(
                dtstart=anchor_lookback,
                rrule_str=rrule_str,
                now=anchor_lookback,
            )
            if dtstart is None:
                entry["reason"] = "rrule produced no next-fire from created_at"
                results.append(entry)
                fail += 1
                continue

            rrule_next = next_fire_from_rrule(
                dtstart=dtstart,
                rrule_str=rrule_str,
                now=now,
            )
            entry["rrule_next_fire"] = rrule_next.isoformat() if rrule_next else None

            if cron_next is None or rrule_next is None:
                entry["verdict"] = "FAIL"
                entry["reason"] = "could not compute one or both next-fires"
                fail += 1
            elif abs((cron_next - rrule_next).total_seconds()) <= 60:
                # Tolerance of 60s — cron is minute-resolution; rrule may
                # land a few seconds off without operational impact.
                entry["verdict"] = "MATCH"
                match += 1
            else:
                entry["verdict"] = "MISMATCH"
                entry["reason"] = (
                    f"next-fire diff = {(rrule_next - cron_next).total_seconds():.0f}s"
                )
                mismatch += 1

            results.append(entry)

        if as_json:
            json.dump(
                {
                    "summary": {
                        "total": len(results),
                        "match": match,
                        "mismatch": mismatch,
                        "fail": fail,
                    },
                    "rows": results,
                },
                sys.stdout,
                indent=2,
                default=str,
            )
            sys.stdout.write("\n")
            return

        # Human-readable output
        self.stdout.write(
            f"Dry-run cron → RRULE for {len(results)} binding(s) — "
            f"MATCH={match} MISMATCH={mismatch} FAIL={fail}"
        )
        self.stdout.write("=" * 80)
        for entry in results:
            self.stdout.write(f"\n[{entry['verdict']}] binding={entry['binding_id']}  "
                              f"workspace={entry['workspace']!r}  enabled={entry['enabled']}")
            self.stdout.write(f"  cron:   {entry['cron']!r}")
            if entry["rrule"]:
                self.stdout.write(f"  rrule:  {entry['rrule']}")
            if entry["cron_next_fire"]:
                self.stdout.write(f"  cron next-fire:  {entry['cron_next_fire']}")
            if entry["rrule_next_fire"]:
                self.stdout.write(f"  rrule next-fire: {entry['rrule_next_fire']}")
            if entry["reason"]:
                self.stdout.write(f"  reason: {entry['reason']}")

        self.stdout.write("\n" + "=" * 80)
        self.stdout.write(
            f"Totals — MATCH={match}  MISMATCH={mismatch}  FAIL={fail}"
        )
        if mismatch or fail:
            self.stdout.write(
                "\nNon-MATCH bindings need manual review before deploying migration 0140."
            )
