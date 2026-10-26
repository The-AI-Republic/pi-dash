# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Collapse duplicate dev-machine records left by older clients.

Usage::

    python manage.py merge_duplicate_dev_machines                 # dry run
    python manage.py merge_duplicate_dev_machines --apply
    python manage.py merge_duplicate_dev_machines --stale-days 30 --owner someone@example.com

Phase 1 merges every (owner, fingerprint) group with more than one active
record into the one with the most live runners (ties: oldest).

Phase 2 handles records without a fingerprint: an active, fingerprint-less
record with no live runners that has not been seen for ``--stale-days``
is merged into another active record of the same owner and host label
(preferring one with runners, then the most recently seen).

Merges go through :func:`pi_dash.runner.services.dev_machines.merge_dev_machines`.
"""

from __future__ import annotations

from collections import defaultdict
from datetime import timedelta

from django.core.management.base import BaseCommand
from django.db import transaction
from django.db.models import Count, Q
from django.utils import timezone

from pi_dash.runner.models import DevMachine
from pi_dash.runner.services.dev_machines import merge_dev_machines


def _active_machines(owner_email):
    qs = DevMachine.objects.filter(revoked_at__isnull=True, merged_into__isnull=True).select_related("owner")
    if owner_email:
        qs = qs.filter(owner__email__iexact=owner_email)
    return qs.annotate(live_runners=Count("runners", filter=Q(runners__revoked_at__isnull=True), distinct=True))


class Command(BaseCommand):
    help = "Merge duplicate DevMachine records for the same computer (dry run unless --apply)."

    def add_arguments(self, parser):
        parser.add_argument("--apply", action="store_true", help="Perform the merges (default: dry run).")
        parser.add_argument(
            "--stale-days",
            type=int,
            default=30,
            help="Phase 2: only merge fingerprint-less records not seen for this many days (default 30).",
        )
        parser.add_argument("--owner", default="", help="Only consider machines owned by this email.")

    def handle(self, *args, apply: bool = False, stale_days: int = 30, owner: str = "", **options):
        plans = self._plan_fingerprint_groups(owner)
        # Phase 2 must not target (or re-merge) records phase 1 folds away.
        merged_away = {other.pk for _, others, _ in plans for other in others}
        plans += self._plan_stale_leftovers(owner, stale_days, exclude=merged_away)
        if not plans:
            self.stdout.write(self.style.SUCCESS("No duplicate dev machines found."))
            return

        for survivor, others, reason in plans:
            self.stdout.write(
                f"{'MERGE' if apply else 'WOULD MERGE'} owner={survivor.owner.email} "
                f"host_label={survivor.host_label!r} reason={reason}"
            )
            self.stdout.write(f"  survivor {survivor.pk} runners={survivor.live_runners}")
            for other in others:
                self.stdout.write(f"  merge    {other.pk} runners={other.live_runners} host_label={other.host_label!r}")

        merged = sum(len(others) for _, others, _ in plans)
        if not apply:
            self.stdout.write(
                self.style.WARNING(
                    f"Dry run: {merged} record(s) in {len(plans)} group(s) would be merged. Re-run with --apply."
                )
            )
            return

        for survivor, others, reason in plans:
            with transaction.atomic():
                merge_dev_machines(survivor, others, reason=f"command:{reason}")
        self.stdout.write(self.style.SUCCESS(f"Merged {merged} record(s) in {len(plans)} group(s)."))

    def _plan_fingerprint_groups(self, owner_email):
        groups = defaultdict(list)
        for machine in _active_machines(owner_email).exclude(fingerprint=""):
            groups[(machine.owner_id, machine.fingerprint)].append(machine)
        plans = []
        for machines in groups.values():
            if len(machines) < 2:
                continue
            survivor = sorted(machines, key=lambda m: (-m.live_runners, m.created_at))[0]
            plans.append((survivor, [m for m in machines if m.pk != survivor.pk], "same_fingerprint"))
        return plans

    def _plan_stale_leftovers(self, owner_email, stale_days, *, exclude):
        cutoff = timezone.now() - timedelta(days=stale_days)
        by_host = defaultdict(list)
        for machine in _active_machines(owner_email).exclude(host_label="").exclude(pk__in=exclude):
            by_host[(machine.owner_id, machine.host_label)].append(machine)
        plans = []
        for machines in by_host.values():
            if len(machines) < 2:
                continue
            leftovers = [
                m
                for m in machines
                if not m.fingerprint and m.live_runners == 0 and (m.last_seen_at is None or m.last_seen_at < cutoff)
            ]
            targets = [m for m in machines if m not in leftovers]
            if not targets:
                # Every record is a stale leftover; keep the most recent one.
                targets = sorted(leftovers, key=_target_rank)[:1]
                leftovers = [m for m in leftovers if m.pk != targets[0].pk]
            if not leftovers:
                continue
            target = sorted(targets, key=_target_rank)[0]
            plans.append((target, leftovers, "stale_same_host_label"))
        return plans


def _target_rank(machine):
    """Prefer machines with runners, then the most recently seen."""
    seen = machine.last_seen_at.timestamp() if machine.last_seen_at else float("-inf")
    return (machine.live_runners == 0, -seen)
