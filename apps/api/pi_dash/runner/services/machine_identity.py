# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Machine identity: engine runner names, machine rename, merge, replace.

``.ai_design/built_engine_as_runner/design.md`` §4.10, §5 and §13.

- :func:`engine_runner_name` assigns ``builtin-<machine>-<workspace>-<project>``
  names, unique across the machine's live runners (one daemon serves every
  workspace, and rejects duplicate names across its whole config) and per pod.
- :func:`update_machine_controls` is the owner-only rename / engine-control
  write behind both machine ``PATCH`` endpoints.
- :func:`merge_dev_machine` moves a replaced or legacy machine's runners into
  the machine that now serves the computer, with the collision rule of §13.
- :func:`replace_candidates` lists the machines a fresh sign-in may replace.
"""

from __future__ import annotations

import hashlib
import logging
import re
from datetime import timedelta
from typing import Iterable, Optional

from django.db import transaction
from django.db.models import Exists, OuterRef
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    DevMachine,
    EngineCredentialRevokeReason,
    MachineSession,
    MachineToken,
    Runner,
    RunnerProvisioning,
    RunnerSession,
    RunnerStatus,
)

logger = logging.getLogger(__name__)

ENGINE_NAME_PREFIX = "builtin"
#: ``Runner.name`` max_length.
RUNNER_NAME_MAX = 128
_HASH_LEN = 8
_SLUG_RE = re.compile(r"[^a-z0-9]+")

#: A machine session seen within this window is live (matches the runner
#: list's control-presence window: three missed 25 s polls).
LIVE_SESSION_WINDOW = timedelta(seconds=90)

MACHINE_LABEL_MAX = 128


# ---------------------------------------------------------------------------
# Naming
# ---------------------------------------------------------------------------


def _slug(value: str) -> str:
    return _SLUG_RE.sub("-", (value or "").lower()).strip("-")


def machine_slug(machine: DevMachine) -> str:
    """The machine part of an engine runner name: its hostname, slugged."""
    return _slug(machine.host_label) or _slug(machine.label) or str(machine.pk).split("-")[0]


def engine_runner_base_name(machine: DevMachine, workspace, project) -> str:
    """``builtin-<machine-slug>-<workspace-slug>-<project-identifier>``,
    lower-cased and cut to the field length with a short hash of the project
    id when too long. The workspace is part of the name because project
    identifiers are unique only within a workspace."""
    parts = [ENGINE_NAME_PREFIX, machine_slug(machine), _slug(workspace.slug), _slug(project.identifier)]
    base = "-".join(p for p in parts if p)
    if len(base) <= RUNNER_NAME_MAX:
        return base
    digest = hashlib.sha256(str(project.pk).encode()).hexdigest()[:_HASH_LEN]
    return f"{base[: RUNNER_NAME_MAX - _HASH_LEN - 1].rstrip('-')}-{digest}"


def _with_suffix(base: str, n: int) -> str:
    suffix = f"-{n}"
    return f"{base[: RUNNER_NAME_MAX - len(suffix)].rstrip('-')}{suffix}"


def _taken_names(machine: DevMachine, pod, exclude_runner_id=None) -> set:
    runners = Runner._base_manager
    machine_names = runners.filter(dev_machine_id=machine.pk, revoked_at__isnull=True)
    # ``runner_unique_name_per_pod`` covers every row in the pod, revoked too.
    pod_names = runners.filter(pod_id=pod.pk) if pod is not None else runners.none()
    if exclude_runner_id is not None:
        machine_names = machine_names.exclude(pk=exclude_runner_id)
        pod_names = pod_names.exclude(pk=exclude_runner_id)
    return set(machine_names.values_list("name", flat=True)) | set(pod_names.values_list("name", flat=True))


def unique_runner_name(base: str, machine: DevMachine, pod, exclude_runner_id=None) -> str:
    """``base``, or ``base-2``, ``base-3``… — the first not used by a live
    runner of ``machine`` or by any runner in ``pod``."""
    taken = _taken_names(machine, pod, exclude_runner_id)
    if base not in taken:
        return base
    n = 2
    while _with_suffix(base, n) in taken:
        n += 1
    return _with_suffix(base, n)


def engine_runner_name(machine: DevMachine, workspace, project, pod, exclude_runner_id=None) -> str:
    """The server-assigned name for an engine runner (design §5)."""
    return unique_runner_name(engine_runner_base_name(machine, workspace, project), machine, pod, exclude_runner_id)


# ---------------------------------------------------------------------------
# Owner controls (rename, engine pause, team runs)
# ---------------------------------------------------------------------------


class MachineControlError(ValueError):
    def __init__(self, code: str, http_status: int = 400):
        super().__init__(code)
        self.code = code
        self.http_status = http_status


def parse_machine_controls(data, allowed: Iterable[str]) -> dict:
    """Validate a machine ``PATCH`` body against ``allowed`` field names."""
    allowed = tuple(allowed)
    unknown = sorted(set(data.keys()) - set(allowed) - {"workspace"})
    if unknown:
        raise MachineControlError("only these fields can be changed here: " + ", ".join(allowed))
    values = {}
    for field in allowed:
        if field not in data:
            continue
        value = data.get(field)
        if field == "label":
            if not isinstance(value, str):
                raise MachineControlError("label must be a string")
            value = " ".join(value.split())
            if len(value) > MACHINE_LABEL_MAX:
                raise MachineControlError(f"label must be at most {MACHINE_LABEL_MAX} characters")
        elif not isinstance(value, bool):
            raise MachineControlError(f"{field} must be a boolean")
        values[field] = value
    return values


def update_machine_controls(machine_id, user, values: dict) -> DevMachine:
    """Apply owner controls to a machine; raises :class:`MachineControlError`.

    ``label`` renames the machine, and with it every engine runner's display
    name ("Built-in engine · <label>"); an empty label falls back to the
    hostname. Engine pause changes release or drain the engines' non-strict
    pins in the same transaction.
    """
    from pi_dash.runner.services.dispatch_rules import on_engine_pause_changed

    with transaction.atomic():
        machine = DevMachine.objects.select_for_update().filter(pk=machine_id).first()
        if machine is None:
            raise MachineControlError("not found", 404)
        if machine.owner_id != user.id:
            raise MachineControlError("forbidden", 403)
        if machine.revoked_at is not None:
            raise MachineControlError("dev_machine_revoked", 409)
        values = dict(values)
        if "label" in values and not values["label"]:
            values["label"] = (machine.host_label or "")[:MACHINE_LABEL_MAX]
        was_paused = machine.engine_paused
        changed = [field for field, value in values.items() if getattr(machine, field) != value]
        for field in changed:
            setattr(machine, field, values[field])
        if changed:
            machine.save(update_fields=[*changed, "updated_at"])
            on_engine_pause_changed(machine, was_paused=was_paused)
    return machine


# ---------------------------------------------------------------------------
# Live session
# ---------------------------------------------------------------------------


def live_session_q():
    return Exists(
        MachineSession.objects.filter(
            dev_machine_id=OuterRef("pk"),
            revoked_at__isnull=True,
            last_seen_at__gte=timezone.now() - LIVE_SESSION_WINDOW,
        )
    )


def has_live_session(machine: DevMachine) -> bool:
    return MachineSession.objects.filter(
        dev_machine=machine,
        revoked_at__isnull=True,
        last_seen_at__gte=timezone.now() - LIVE_SESSION_WINDOW,
    ).exists()


# ---------------------------------------------------------------------------
# Replace prompt (§4.10)
# ---------------------------------------------------------------------------


def replace_candidates(machine: DevMachine):
    """Machines a fresh sign-in on ``machine`` may replace: the same owner's
    other machines with the same ``host_label``, not revoked or merged, and
    with no live session. Two computers can share a hostname, so this only
    feeds the "Is this <label>? Replace it" prompt; it never merges."""
    if not machine.host_label:
        return DevMachine.objects.none()
    return (
        DevMachine.objects.filter(
            owner_id=machine.owner_id,
            host_label=machine.host_label,
            revoked_at__isnull=True,
            merged_into__isnull=True,
        )
        .exclude(pk=machine.pk)
        .annotate(_live=live_session_q())
        .filter(_live=False)
        .order_by("-last_seen_at", "-created_at")
    )


# ---------------------------------------------------------------------------
# Merge (§13)
# ---------------------------------------------------------------------------


class MergeError(Exception):
    def __init__(self, code: str, http_status: int):
        super().__init__(code)
        self.code = code
        self.http_status = http_status


def _hand_over_to_survivor(loser: Runner, survivor: Runner) -> set:
    """Collision: move ``loser``'s issues and queued strict runs to
    ``survivor`` (runner **and** pod: the matcher scans only a runner's
    current pod, and the two can sit in different pods of the project), and
    release its queued non-strict pins to their own pod. Returns the pods to
    drain."""
    from pi_dash.db.models.issue import Issue

    Issue.all_objects.filter(assigned_runner=loser).update(assigned_runner=survivor, assigned_pod=survivor.pod_id)
    strict = AgentRun.objects.filter(pinned_runner=loser, status=AgentRunStatus.QUEUED, pin_strict=True)
    strict_pods = set(strict.values_list("pod_id", flat=True))
    strict.update(pinned_runner=survivor, pod_id=survivor.pod_id)
    loose = AgentRun.objects.filter(pinned_runner=loser, status=AgentRunStatus.QUEUED, pin_strict=False)
    loose_pods = set(loose.values_list("pod_id", flat=True))
    loose.update(pinned_runner=None)
    return {survivor.pod_id} | strict_pods | loose_pods


def merge_dev_machine(*, user, source_id, target_id) -> dict:
    """Merge machine ``source_id`` into ``target_id`` in one transaction.

    Both must belong to ``user``; the source must have no live session and
    the target must be live (not revoked or merged away). Every live runner
    of the source moves to the target with its id, pod, pins and queued runs.
    When both have a live engine runner for one project, the target's
    survives and the source's work is handed to it before the source's
    runner is revoked. The source's tokens and engine credential are
    revoked, and the source is marked revoked with ``merged_into``.

    Idempotent: a retry for a source already merged into the same target
    returns the stored result; into another target it is refused.
    """
    from pi_dash.runner.services.engine_credentials import revoke_engine_credential
    from pi_dash.runner.services.matcher import drain_pod_by_id

    if str(source_id) == str(target_id):
        raise MergeError("cannot_merge_into_self", 400)

    with transaction.atomic():
        locked = {
            str(m.pk): m
            for m in DevMachine.objects.select_for_update().filter(pk__in=[source_id, target_id]).order_by("pk")
        }
        source = locked.get(str(source_id))
        target = locked.get(str(target_id))
        if source is None or target is None or source.owner_id != user.id or target.owner_id != user.id:
            raise MergeError("dev_machine_not_found", 404)
        if source.merged_into_id is not None:
            if str(source.merged_into_id) == str(target.pk):
                return dict(source.merge_result)
            raise MergeError("already_merged_into_another_machine", 409)
        if source.revoked_at is not None:
            raise MergeError("source_machine_revoked", 409)
        if target.revoked_at is not None or target.merged_into_id is not None:
            raise MergeError("target_machine_not_live", 409)
        if has_live_session(source):
            raise MergeError("source_machine_has_live_session", 409)

        now = timezone.now()
        survivors = {
            r.pod.project_id: r
            for r in Runner.objects.select_related("pod")
            .filter(dev_machine=target, provisioning=RunnerProvisioning.DESKTOP_BUNDLED, revoked_at__isnull=True)
            .order_by("created_at")
        }
        runners = list(
            Runner.objects.select_for_update()
            .select_related("pod__project", "workspace")
            .filter(dev_machine=source, revoked_at__isnull=True)
            .order_by("created_at")
        )
        moved, collisions, drain_pods = [], [], set()
        for runner in runners:
            project_id = runner.pod.project_id
            survivor = survivors.get(project_id) if runner.is_engine else None
            if survivor is not None:
                drain_pods |= _hand_over_to_survivor(runner, survivor)
                runner.revoke(reason="dev_machine_merged")
                collisions.append({"revoked_runner_id": str(runner.pk), "survivor_runner_id": str(survivor.pk)})
                continue
            if runner.is_engine:
                name = engine_runner_name(target, runner.workspace, runner.pod.project, runner.pod, runner.pk)
                survivors[project_id] = runner
            else:
                name = unique_runner_name(runner.name, target, runner.pod, runner.pk)
            # The source daemon is gone: its runner sessions die with it and
            # the target's daemon attaches afresh.
            RunnerSession.objects.filter(runner=runner, revoked_at__isnull=True).update(
                revoked_at=now, revoked_reason="dev_machine_merged"
            )
            Runner.objects.filter(pk=runner.pk).update(
                dev_machine=target,
                host_label=target.host_label or runner.host_label,
                name=name,
                status=RunnerStatus.OFFLINE,
                updated_at=now,
            )
            moved.append(str(runner.pk))

        MachineToken.objects.filter(dev_machine=source, revoked_at__isnull=True).update(revoked_at=now)
        MachineSession.objects.filter(dev_machine=source, revoked_at__isnull=True).update(
            revoked_at=now, revoked_reason="dev_machine_merged"
        )
        result = {
            "source_dev_machine_id": str(source.pk),
            "target_dev_machine_id": str(target.pk),
            "merged_at": now.isoformat(),
            "moved_runner_ids": moved,
            "collisions": collisions,
        }
        source.revoked_at = now
        source.merged_into = target
        source.merge_result = result
        source.save(update_fields=["revoked_at", "merged_into", "merge_result", "updated_at"])
        # After ``revoked_at``: a merged machine gets no automatic pause.
        revoke_engine_credential(source, reason=EngineCredentialRevokeReason.MACHINE_MERGED)
        for pod_id in drain_pods:
            if pod_id is not None:
                transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))

    logger.info(
        "dev_machine.merged user=%s source=%s target=%s moved=%s collisions=%s",
        user.id,
        source.pk,
        target.pk,
        len(moved),
        len(collisions),
    )
    return result


def live_engine_runner_for(machine: DevMachine, project, *, lock: bool = False) -> Optional[Runner]:
    """The machine's live engine runner on ``project`` (at most one, §5)."""
    qs = Runner.objects.filter(
        dev_machine=machine,
        pod__project_id=project.pk,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        revoked_at__isnull=True,
    )
    if lock:
        qs = qs.select_for_update()
    return qs.order_by("created_at").first()
