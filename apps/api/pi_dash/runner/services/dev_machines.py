# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dev-machine identity: one record per computer.

Clients identify their machine with a random ``dev_machine_id`` kept in
``config.toml`` and, when they can read one, a ``machine_fingerprint``:
the lowercase hex SHA-256 of ``"pidash-machine-v1\\0" + <OS machine id>``.
The id alone is not stable (``pidash remove``, reinstalls and the desktop
app's old per-workspace configs all mint new ones), so the fingerprint is
what collapses duplicates. :func:`resolve_dev_machine` is the single
resolution rule every enrollment path uses; :func:`merge_dev_machines`
folds duplicates into a survivor.
"""

from __future__ import annotations

import logging
import re
import uuid as _uuid
from typing import Iterable, Optional

from django.db import IntegrityError, transaction
from django.db.models import Count, Q
from django.utils import timezone

from pi_dash.runner.models import (
    DesktopHandoffKey,
    DevMachine,
    MachineSession,
    MachineToken,
    Runner,
    RunnerProvisioning,
)

logger = logging.getLogger(__name__)

_FINGERPRINT_RE = re.compile(r"^[0-9a-f]{64}$")

SUPERVISOR_SERVICE = "service"
SUPERVISOR_DESKTOP = "desktop"
SUPERVISORS = frozenset({SUPERVISOR_SERVICE, SUPERVISOR_DESKTOP})

# ``MachineSession.revoked_reason`` for sessions of a merged-away record.
MERGED_SESSION_REASON = "merged"


class DevMachineOwnershipError(Exception):
    """``dev_machine_id`` names a record owned by someone else."""


class InvalidMachineFingerprint(ValueError):
    pass


def parse_machine_fingerprint(raw) -> str:
    """Return the validated fingerprint, or ``""`` when absent.

    Raises :class:`InvalidMachineFingerprint` for a present but malformed
    value; callers answer 400 ``invalid_machine_fingerprint``.
    """
    if raw is None:
        return ""
    if not isinstance(raw, str):
        raise InvalidMachineFingerprint
    value = raw.strip()
    if not value:
        return ""
    if not _FINGERPRINT_RE.match(value):
        raise InvalidMachineFingerprint
    return value


def follow_merged(machine: DevMachine) -> DevMachine:
    """Walk ``merged_into`` to the record that absorbed ``machine``."""
    seen = {machine.pk}
    while machine.merged_into_id is not None and machine.merged_into_id not in seen:
        seen.add(machine.merged_into_id)
        nxt = DevMachine.objects.select_for_update().filter(pk=machine.merged_into_id).first()
        if nxt is None:
            break
        machine = nxt
    return machine


def merged_survivor_id(dev_machine_id) -> Optional[_uuid.UUID]:
    """Survivor id when ``dev_machine_id`` names a merged record, else None."""
    machine = DevMachine.objects.filter(pk=dev_machine_id).first()
    if machine is None or machine.merged_into_id is None:
        return None
    seen = {machine.pk}
    while machine.merged_into_id is not None and machine.merged_into_id not in seen:
        seen.add(machine.merged_into_id)
        nxt = DevMachine.objects.filter(pk=machine.merged_into_id).first()
        if nxt is None:
            break
        machine = nxt
    return machine.pk


def _is_active(machine: DevMachine) -> bool:
    return machine.revoked_at is None and machine.merged_into_id is None


def _touch(machine: DevMachine, *, host_label: str) -> DevMachine:
    host_label = (host_label or "").strip()[:255]
    update_fields = ["last_seen_at", "updated_at"]
    machine.last_seen_at = timezone.now()
    if host_label and machine.host_label != host_label:
        machine.host_label = host_label
        update_fields.append("host_label")
    if host_label and not machine.label:
        machine.label = host_label[:128]
        update_fields.append("label")
    machine.save(update_fields=update_fields)
    return machine


def _candidates(*, user, fingerprint: str) -> list[DevMachine]:
    if not fingerprint:
        return []
    return list(
        DevMachine.objects.select_for_update()
        .filter(
            owner=user,
            fingerprint=fingerprint,
            revoked_at__isnull=True,
            merged_into__isnull=True,
        )
        .order_by("created_at")
    )


def _pick_survivor(machines: list[DevMachine]) -> DevMachine:
    """Rule 4: most non-revoked runners, ties -> oldest ``created_at``."""
    counts = dict(
        Runner.objects.filter(dev_machine__in=machines, revoked_at__isnull=True)
        .values("dev_machine_id")
        .annotate(n=Count("id"))
        .values_list("dev_machine_id", "n")
    )
    return sorted(machines, key=lambda m: (-counts.get(m.pk, 0), m.created_at))[0]


def _absorb(survivor: DevMachine, *, fingerprint: str, others: list[DevMachine], host_label: str) -> DevMachine:
    """Rule 6: merge ``others`` in, then stamp the fingerprint and touch."""
    others = [m for m in others if m.pk != survivor.pk]
    if others:
        merge_dev_machines(survivor, others, reason="fingerprint")
    if fingerprint and not survivor.fingerprint:
        survivor.fingerprint = fingerprint
        survivor.save(update_fields=["fingerprint", "updated_at"])
    return _touch(survivor, host_label=host_label)


def resolve_dev_machine(
    *,
    user,
    dev_machine_id: Optional[_uuid.UUID],
    host_label: str,
    fingerprint: str,
    provisioning=None,
) -> DevMachine:
    """Resolve the one active ``DevMachine`` for this computer.

    Must be called inside a transaction. See the module docstring and the
    contract's resolution rule:

    1. A known ``dev_machine_id`` owned by someone else raises
       :class:`DevMachineOwnershipError`; a merged one is followed to its
       survivor.
    2. Candidates are the owner's active records with the same fingerprint.
    3. The id's record survives when active and its fingerprint is empty
       or equal (a different fingerprint means a copied config).
    4. Otherwise the candidate with the most live runners (then oldest).
    5. Otherwise a new record (client id if unused, else a fresh UUID).
    6. Stamp the fingerprint, touch, merge the other candidates.

    Without both id and fingerprint, older clients fall back to the
    owner's oldest active record with the same ``host_label`` (and
    ``provisioning``, when given), as before.
    """
    try:
        with transaction.atomic():
            return _resolve(
                user=user,
                dev_machine_id=dev_machine_id,
                host_label=host_label,
                fingerprint=fingerprint,
                provisioning=provisioning,
            )
    except IntegrityError:
        # A concurrent request created/stamped the same (owner, fingerprint)
        # first. Its row is committed now, so a second pass finds it.
        return _resolve(
            user=user,
            dev_machine_id=dev_machine_id,
            host_label=host_label,
            fingerprint=fingerprint,
            provisioning=provisioning,
        )


def _resolve(*, user, dev_machine_id, host_label, fingerprint, provisioning) -> DevMachine:
    host_label = (host_label or "").strip()[:255]
    fingerprint = fingerprint or ""

    by_id: Optional[DevMachine] = None
    id_taken = False
    if dev_machine_id is not None:
        by_id = DevMachine.objects.select_for_update().filter(pk=dev_machine_id).first()
        if by_id is not None:
            id_taken = True
            if by_id.owner_id != user.id:
                raise DevMachineOwnershipError
            if by_id.merged_into_id is not None:
                by_id = follow_merged(by_id)
                if by_id.owner_id != user.id:
                    raise DevMachineOwnershipError

    candidates = _candidates(user=user, fingerprint=fingerprint)

    if by_id is not None and _is_active(by_id) and (not by_id.fingerprint or by_id.fingerprint == fingerprint):
        return _absorb(by_id, fingerprint=fingerprint, others=candidates, host_label=host_label)
    if by_id is not None and _is_active(by_id) and fingerprint and by_id.fingerprint != fingerprint:
        logger.info(
            "dev_machine.fingerprint_mismatch owner=%s dev_machine=%s; treating as copied config",
            user.id,
            by_id.pk,
        )

    if candidates:
        survivor = _pick_survivor(candidates)
        return _absorb(survivor, fingerprint=fingerprint, others=candidates, host_label=host_label)

    if dev_machine_id is None and not fingerprint and host_label:
        legacy = DevMachine.objects.select_for_update().filter(
            owner=user,
            host_label=host_label,
            revoked_at__isnull=True,
            merged_into__isnull=True,
        )
        if provisioning is not None:
            legacy = legacy.filter(provisioning=provisioning)
        existing = legacy.order_by("created_at").first()
        if existing is not None:
            return _touch(existing, host_label=host_label)

    create_kwargs = {
        "owner": user,
        "host_label": host_label,
        "label": host_label[:128],
        "fingerprint": fingerprint,
        "provisioning": provisioning or RunnerProvisioning.MANUAL,
        "last_seen_at": timezone.now(),
    }
    if dev_machine_id is not None and not id_taken:
        create_kwargs["id"] = dev_machine_id
    return DevMachine.objects.create(**create_kwargs)


def claim_fingerprint(machine: DevMachine, *, fingerprint: str) -> DevMachine:
    """Session-open variant of rule 6 for an already-authenticated machine.

    Stamps ``fingerprint`` on ``machine`` when empty and merges the owner's
    other active records with that fingerprint into it. A machine whose own
    fingerprint differs is left alone (copied config; the next
    ``pidash auth login`` sorts it out). Must run inside a transaction.
    """
    if not fingerprint:
        return machine
    locked = DevMachine.objects.select_for_update().get(pk=machine.pk)
    if locked.fingerprint and locked.fingerprint != fingerprint:
        logger.info(
            "dev_machine.session_fingerprint_mismatch dev_machine=%s; not merging",
            locked.pk,
        )
        return locked
    others = [m for m in _candidates(user=locked.owner, fingerprint=fingerprint) if m.pk != locked.pk]
    if others:
        merge_dev_machines(locked, others, reason="session_fingerprint")
    if not locked.fingerprint:
        locked.fingerprint = fingerprint
        locked.save(update_fields=["fingerprint", "updated_at"])
    return locked


def merge_dev_machines(survivor: DevMachine, others: Iterable[DevMachine], *, reason: str = "") -> int:
    """Fold ``others`` into ``survivor`` in one transaction.

    - runners move to the survivor;
    - each active machine token moves when the survivor has no active
      token for that workspace, otherwise it is revoked (one active token
      per (workspace, dev_machine));
    - active machine sessions of merged records are revoked (``merged``);
    - active desktop handoff keys move to the survivor;
    - merged records get ``merged_into`` + ``revoked_at``.

    Returns the number of records merged.
    """
    other_ids = [m.pk for m in others if m.pk != survivor.pk]
    if not other_ids:
        return 0
    with transaction.atomic():
        now = timezone.now()
        locked = list(DevMachine.objects.select_for_update().filter(pk__in=other_ids).order_by("created_at"))
        other_ids = [m.pk for m in locked]
        runner_count = Runner.objects.filter(dev_machine_id__in=other_ids).update(dev_machine=survivor)

        taken_workspaces = set(
            MachineToken.objects.select_for_update()
            .filter(dev_machine=survivor, revoked_at__isnull=True)
            .values_list("workspace_id", flat=True)
        )
        moved = revoked = 0
        for token in (
            MachineToken.objects.select_for_update()
            .filter(dev_machine_id__in=other_ids, revoked_at__isnull=True)
            .order_by("-created_at")
        ):
            if token.workspace_id in taken_workspaces:
                token.revoked_at = now
                token.save(update_fields=["revoked_at"])
                revoked += 1
            else:
                token.dev_machine = survivor
                token.save(update_fields=["dev_machine"])
                taken_workspaces.add(token.workspace_id)
                moved += 1

        MachineSession.objects.filter(dev_machine_id__in=other_ids, revoked_at__isnull=True).update(
            revoked_at=now, revoked_reason=MERGED_SESSION_REASON
        )
        DesktopHandoffKey.objects.filter(dev_machine_id__in=other_ids, revoked_at__isnull=True).update(
            dev_machine=survivor
        )
        # Keep chains flat so an old id resolves in one hop.
        DevMachine.objects.filter(merged_into_id__in=other_ids).update(merged_into=survivor, updated_at=now)
        DevMachine.objects.filter(pk__in=other_ids).update(
            merged_into=survivor,
            revoked_at=now,
            updated_at=now,
        )

    logger.info(
        "dev_machine.merged survivor=%s merged=%s owner=%s reason=%s runners=%s tokens_moved=%s tokens_revoked=%s",
        survivor.pk,
        [str(pk) for pk in other_ids],
        survivor.owner_id,
        reason or "-",
        runner_count,
        moved,
        revoked,
    )
    return len(other_ids)


def active_dev_machines_q() -> Q:
    """Filter for records that are neither revoked nor merged."""
    return Q(revoked_at__isnull=True, merged_into__isnull=True)
