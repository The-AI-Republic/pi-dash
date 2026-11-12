# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dispatch rules for built-in engine runners.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §7-§9
(revision 4.0). An engine gets issue work only as a member of its pod; there
is no way to send an issue, or one run of it, to a specific engine
(dedicating an issue to a runner belongs to the dedicated-runner design, HA
§23). This module is the one place that answers:

- may this runner take a run with this provenance (the usage rule, §8);
- does an engine take pod work now (``pod_member`` and not paused, §7);
- may a continuation be pinned to this runner for repo locality (§7);
- what happens to an engine's queued locality pins when it stops taking pod
  work (pod work off, owner pause, automatic pause).

The queryset form of the usage rule lives in
:func:`pi_dash.runner.services.permissions.filter_runs_usable_by_runner`
(dispatch); the Python form here (:func:`runner_may_take`) serves preflight,
and must stay equivalent to it.
"""

from __future__ import annotations

import logging
from typing import Iterable, Optional

from django.db import transaction

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerPodDispatch,
    RunnerProvisioning,
)

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Usage rule (§8)
# ---------------------------------------------------------------------------


def _issue_people_ids(issue) -> set:
    """Users whose involvement in ``issue`` lets their runner take its work:
    the creator and every live (non-soft-deleted) assignee."""
    from pi_dash.db.models.issue import IssueAssignee

    ids = set()
    if issue is None:
        return ids
    if issue.created_by_id is not None:
        ids.add(issue.created_by_id)
    ids.update(IssueAssignee.objects.filter(issue=issue).values_list("assignee_id", flat=True))
    ids.discard(None)
    return ids


def engine_team_runs_enabled(runner: Runner) -> bool:
    if runner.dev_machine_id is None:
        return False
    return bool(runner.dev_machine.engine_team_runs)


def runner_may_take(
    runner: Runner,
    *,
    created_by_id,
    human: bool,
    issue=None,
    scheduler_binding=None,
) -> bool:
    """Python form of the usage rule in ``filter_runs_usable_by_runner``.

    Own-agent runner (and an engine whose owner opted in to teammates' runs):
    the owner created the run, or the run is on an issue the owner created or
    is assigned to, or comes from a scheduler binding the owner authored.

    Engine runner (PO-3): the owner created the run; or the run is automatic
    (tick, scheduler) and is on such an issue or from such a binding. A
    teammate's click is refused unless ``DevMachine.engine_team_runs`` is on.
    """
    owner_id = runner.owner_id
    if created_by_id is not None and created_by_id == owner_id:
        return True
    involved = owner_id in _issue_people_ids(issue) or (
        scheduler_binding is not None and getattr(scheduler_binding, "actor_id", None) == owner_id
    )
    if not involved:
        return False
    if not runner.is_engine:
        return True
    return (not human) or engine_team_runs_enabled(runner)


# ---------------------------------------------------------------------------
# Pod work and locality pins (§7)
# ---------------------------------------------------------------------------


def engine_takes_pod_work(runner: Runner) -> bool:
    """An engine takes locality pins and unpinned pod work only while it is a
    pod member and its machine's engine is not paused (§7)."""
    return runner.pod_dispatch == RunnerPodDispatch.POD_MEMBER and not runner.engine_paused


def locality_pin_allowed(runner: Optional[Runner]) -> bool:
    """May a continuation be pinned to ``runner`` for repo locality?

    Own-agent runners: yes, as today. Engines: only while they are a
    ``pod_member`` and not paused; otherwise the follow-up is created
    unpinned in the pod (§7)."""
    if runner is None:
        return False
    if not runner.is_engine:
        return True
    return engine_takes_pod_work(runner)


def release_locality_pins(runner_ids: Iterable) -> int:
    """Send QUEUED locality-pinned runs on ``runner_ids`` back to their pod.

    Used when an engine stops taking pod work (pod work turned off, owner
    pause, automatic pause), so none waits for the offline-pin sweep. The
    run's ``pod`` is unchanged. Legacy ``managed_runner`` runs are left
    pinned: only the engine they are pinned to serves them (§13), and their
    own 12 h expiry applies (§9).
    Must be called inside the caller's transaction; drains run on commit.
    """
    from pi_dash.runner.services.matcher import drain_pod_by_id

    runner_ids = list(runner_ids)
    if not runner_ids:
        return 0
    runs = list(
        AgentRun.objects.select_for_update(of=("self",))
        .select_related("parent_run")
        .filter(
            pinned_runner_id__in=runner_ids,
            status=AgentRunStatus.QUEUED,
            executor_kind=AgentExecutorKind.LOCAL_RUNNER,
            pin_kind="locality",
        )
    )
    pod_ids = set()
    for run in runs:
        run.pinned_runner = None
        if run.parent_run is not None and run.parent_run.thread_id:
            run.parent_run.thread_id = ""
            run.parent_run.save(update_fields=["thread_id"])
        run.save(update_fields=["pinned_runner"])
        if run.pod_id is not None:
            pod_ids.add(run.pod_id)
    for pod_id in pod_ids:
        transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))
    if runs:
        logger.info("dispatch_rules.release_locality_pins runners=%s released=%d", runner_ids, len(runs))
    return len(runs)


def engine_runner_ids_for_machine(machine_id) -> list:
    return list(
        Runner.objects.filter(
            dev_machine_id=machine_id,
            provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
            revoked_at__isnull=True,
        ).values_list("pk", flat=True)
    )


def set_engine_auto_pause_reason(machine, reason: str, *, keep=(), only_from=None) -> bool:
    """Set or clear (``""``) a machine's automatic engine pause.

    The seam for the heartbeat report and the model-token endpoint (§8).
    Pausing releases the engines' queued locality pins in the same
    transaction; clearing drains them. Returns True when the value changed.

    The conditions are checked on the locked row, so a caller that decided
    on an unlocked read cannot overwrite a reason set meanwhile: nothing
    changes when the current reason is in ``keep``, or, with ``only_from``,
    when it is not in ``only_from``.
    """
    from pi_dash.runner.models import DevMachine, EngineAutoPauseReason

    reason = reason or ""
    if reason not in EngineAutoPauseReason.values:
        raise ValueError(f"unknown engine auto-pause reason {reason!r}")
    with transaction.atomic():
        locked = DevMachine.objects.select_for_update().get(pk=machine.pk)
        current = locked.engine_auto_pause_reason
        if current == reason or current in keep:
            return False
        if only_from is not None and current not in only_from:
            return False
        was_paused = locked.engine_paused
        locked.engine_auto_pause_reason = reason
        locked.save(update_fields=["engine_auto_pause_reason", "updated_at"])
        machine.engine_auto_pause_reason = reason
        on_engine_pause_changed(locked, was_paused=was_paused)
    return True


def on_engine_pause_changed(machine, *, was_paused: bool) -> None:
    """Apply the dispatch side effects of a machine's engine pause flipping."""
    from pi_dash.runner.services.matcher import drain_for_runner_by_id

    runner_ids = engine_runner_ids_for_machine(machine.pk)
    if machine.engine_paused and not was_paused:
        release_locality_pins(runner_ids)
    elif was_paused and not machine.engine_paused:
        for runner_id in runner_ids:
            transaction.on_commit(lambda rid=runner_id: drain_for_runner_by_id(rid))
