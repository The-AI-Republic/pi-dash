# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dispatch rules for assigned runners, strict pins and built-in engines.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §7-§9. This
module is the one place that answers:

- may this runner take a run with this provenance (the usage rule, §8);
- is this runner available to take one now (online, not paused, engine not
  paused);
- which strict pin a new issue run gets from ``Issue.assigned_runner``, and
  whether its creation is refused (§7, §9);
- how ``assigned_runner`` / ``assigned_pod`` stay consistent when a runner
  moves or goes away (§7).

The queryset form of the usage rule lives in
:func:`pi_dash.runner.services.permissions.filter_runs_usable_by_runner`
(dispatch); the Python form here (:func:`runner_may_take`) serves preflight and
run creation, and must stay equivalent to it.
"""

from __future__ import annotations

import logging
import time
from typing import Iterable, Optional

from django.db import transaction
from django.db.models import Q
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerPodDispatch,
    RunnerProvisioning,
    RunnerStatus,
)

logger = logging.getLogger(__name__)

#: Refusal reasons for a run on an issue with ``assigned_runner``.
ASSIGNED_RUNNER_NOT_OWNER = "assigned_runner_not_owner"
ASSIGNED_RUNNER_OFFLINE = "assigned_runner_offline"
#: Error code a queued strict run carries while its runner is away. The value
#: predates this design (it was the managed runner's "waiting for your
#: desktop" code) and is kept so existing clients keep rendering it.
WAITING_FOR_RUNNER = "desktop_not_connected"
#: A queued strict run whose runner was revoked or deleted.
ASSIGNED_RUNNER_REMOVED = "assigned_runner_removed"
#: The ``PATCH`` refusal for moving a runner with assigned issues to another
#: project.
RUNNER_HAS_ASSIGNED_ISSUES = "runner_has_assigned_issues"

#: Issue-activity field used to record changes to ``assigned_runner`` made by
#: the system (revocation), so the feed says why an issue became pod-level.
ASSIGNED_RUNNER_ACTIVITY_FIELD = "assigned_runner"


class AssignedRunnerUnavailable(ValueError):
    """Run creation refused because the issue's assigned runner cannot take it.

    ``code`` is :data:`ASSIGNED_RUNNER_NOT_OWNER` or
    :data:`ASSIGNED_RUNNER_OFFLINE`; ``str(exc)`` is the sentence for the UI.
    A ``ValueError`` so every creation path's existing ``except (ValueError,
    RuntimeError)`` turns it into a refusal rather than a crash.
    """

    def __init__(self, code: str, detail: str, *, runner: Optional[Runner] = None):
        self.code = code
        self.runner_id = getattr(runner, "id", None)
        super().__init__(detail)


def machine_offline_detail(runner: Runner) -> str:
    """The user-facing "<machine> is offline" sentence (design §9)."""
    return f"{runner.machine_display_name} is offline"


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
# Availability
# ---------------------------------------------------------------------------


def runner_is_connected(runner: Runner) -> bool:
    """Online (idle or busy), heartbeat-fresh and not paused.

    Unlike ``Runner.objects.assignable()`` a BUSY runner counts: a run pinned
    to a busy runner simply waits its turn, it is not "offline". A paused
    engine machine counts as offline (§9).
    """
    from pi_dash.runner.services.matcher import HEARTBEAT_GRACE

    if runner.revoked_at is not None or runner.status not in (RunnerStatus.ONLINE, RunnerStatus.BUSY):
        return False
    if runner.paused_at is not None or runner.engine_paused:
        return False
    if runner.last_heartbeat_at is None:
        return False
    return runner.last_heartbeat_at >= timezone.now() - HEARTBEAT_GRACE


def runner_not_connected_q(prefix: str = "") -> Q:
    """Queryset counterpart of ``not runner_is_connected`` for ``prefix``."""
    from pi_dash.runner.models import engine_paused_q
    from pi_dash.runner.services.matcher import HEARTBEAT_GRACE

    cutoff = timezone.now() - HEARTBEAT_GRACE
    return (
        ~Q(**{f"{prefix}status__in": [RunnerStatus.ONLINE, RunnerStatus.BUSY]})
        | Q(**{f"{prefix}last_heartbeat_at__lt": cutoff})
        | Q(**{f"{prefix}last_heartbeat_at__isnull": True})
        | Q(**{f"{prefix}paused_at__isnull": False})
        | Q(**{f"{prefix}revoked_at__isnull": False})
        | engine_paused_q(prefix)
    )


def engine_takes_pod_work(runner: Runner) -> bool:
    """An engine takes non-strict pins and unpinned pod work only while it is
    a pod member and its machine's engine is not paused (§7)."""
    return runner.pod_dispatch == RunnerPodDispatch.POD_MEMBER and not runner.engine_paused


# ---------------------------------------------------------------------------
# Strict pins at run creation (§7, §9)
# ---------------------------------------------------------------------------


def live_assigned_runner(issue) -> Optional[Runner]:
    """The issue's assigned runner, or None when unset, revoked or moved out
    of the issue's project (a stale row never pins)."""
    runner_id = getattr(issue, "assigned_runner_id", None)
    if runner_id is None:
        return None
    runner = Runner.objects.select_related("dev_machine", "pod").filter(pk=runner_id).first()
    if runner is None or runner.is_revoked:
        return None
    if runner.pod_id is None or runner.pod.project_id != issue.project_id:
        return None
    return runner


def strict_pin_fields(*, issue, actor, automatic: bool) -> dict:
    """AgentRun fields for a run on ``issue`` when it has an assigned runner.

    Returns ``{}`` for a pod-level issue. Otherwise
    ``{"pinned_runner": runner, "pin_strict": True}``, plus a waiting reason
    for an automatic run whose runner is away. Raises
    :class:`AssignedRunnerUnavailable` when the run must be refused:

    - the runner's usage rule would never admit it
      (``assigned_runner_not_owner``, PO-10);
    - a person started it and the runner is offline or paused
      (``assigned_runner_offline``, PO-5).

    Automatic runs to an offline runner are queued visibly instead and expire
    after ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`` (§9).
    """
    if issue is None:
        return {}
    runner = live_assigned_runner(issue)
    if runner is None:
        return {}
    human = not automatic
    if not runner_may_take(runner, created_by_id=getattr(actor, "id", None), human=human, issue=issue):
        raise AssignedRunnerUnavailable(
            ASSIGNED_RUNNER_NOT_OWNER,
            f"This issue is assigned to {runner.machine_display_name}, which only runs its owner's work.",
            runner=runner,
        )
    fields = {"pinned_runner": runner, "pin_strict": True}
    if not runner_is_connected(runner):
        if human:
            raise AssignedRunnerUnavailable(
                ASSIGNED_RUNNER_OFFLINE, machine_offline_detail(runner), runner=runner
            )
        fields["error_details"] = {"code": WAITING_FOR_RUNNER}
    return fields


def nonstrict_pin_allowed(runner: Optional[Runner]) -> bool:
    """May a continuation be pinned (non-strictly, for repo locality) to
    ``runner``? Own-agent runners: yes, as today. Engines: only while they take
    pod work (§7)."""
    if runner is None:
        return False
    if not runner.is_engine:
        return True
    return engine_takes_pod_work(runner)


# ---------------------------------------------------------------------------
# Keeping pins and assignments consistent (§7)
# ---------------------------------------------------------------------------


def release_nonstrict_pins(runner_ids: Iterable) -> int:
    """Send QUEUED non-strict runs pinned to ``runner_ids`` back to their pod.

    Used when an engine stops taking pod work (pod work turned off, owner
    pause, automatic pause), so none waits for the offline-pin sweep. Strict
    pins are left alone: they are the run's target, not a preference.
    Must be called inside the caller's transaction; drains run on commit.
    """
    from pi_dash.runner.services.matcher import drain_pod_by_id

    runner_ids = list(runner_ids)
    if not runner_ids:
        return 0
    runs = list(
        AgentRun.objects.select_for_update(of=("self",))
        .select_related("parent_run")
        .filter(pinned_runner_id__in=runner_ids, status=AgentRunStatus.QUEUED, pin_strict=False)
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
        logger.info("dispatch_rules.release_nonstrict_pins runners=%s released=%d", runner_ids, len(runs))
    return len(runs)


def engine_runner_ids_for_machine(machine_id) -> list:
    return list(
        Runner.objects.filter(
            dev_machine_id=machine_id,
            provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
            revoked_at__isnull=True,
        ).values_list("pk", flat=True)
    )


def set_engine_auto_pause_reason(machine, reason: str) -> bool:
    """Set or clear (``""``) a machine's automatic engine pause.

    The seam for the heartbeat report and the model-token endpoint (§8).
    Pausing releases the engines' queued non-strict pins in the same
    transaction; clearing drains them. Returns True when the value changed.
    """
    from pi_dash.runner.models import DevMachine, EngineAutoPauseReason

    reason = reason or ""
    if reason not in EngineAutoPauseReason.values:
        raise ValueError(f"unknown engine auto-pause reason {reason!r}")
    with transaction.atomic():
        locked = DevMachine.objects.select_for_update().get(pk=machine.pk)
        if locked.engine_auto_pause_reason == reason:
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
        release_nonstrict_pins(runner_ids)
    elif was_paused and not machine.engine_paused:
        for runner_id in runner_ids:
            transaction.on_commit(lambda rid=runner_id: drain_for_runner_by_id(rid))


def _record_assignment_activity(issue_ids, *, old_runner: Runner, comment: str) -> None:
    from pi_dash.db.models.issue import Issue, IssueActivity
    from pi_dash.orchestration.workpad import get_agent_system_user

    actor = get_agent_system_user()
    now = time.time()
    rows = [
        IssueActivity(
            issue_id=issue.id,
            project_id=issue.project_id,
            workspace_id=issue.workspace_id,
            verb="updated",
            field=ASSIGNED_RUNNER_ACTIVITY_FIELD,
            old_value=old_runner.name,
            new_value="",
            old_identifier=old_runner.id,
            comment=comment,
            actor=actor,
            epoch=now,
        )
        for issue in Issue.all_objects.filter(pk__in=list(issue_ids)).only("id", "project_id", "workspace_id")
    ]
    if rows:
        IssueActivity.objects.bulk_create(rows)


def detach_removed_runner(runner: Runner) -> None:
    """A runner is being revoked or deleted: fail its queued strict runs and
    make its assigned issues pod-level (§7).

    Queued strict runs fail with ``assigned_runner_removed`` instead of being
    unpinned, so nothing runs on another machine by accident. Issues keep
    their ``assigned_pod`` and lose ``assigned_runner``; the activity feed
    records why. Must be called inside the caller's transaction.
    """
    from pi_dash.db.models.issue import Issue
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    strict_ids = list(
        AgentRun.objects.select_for_update()
        .filter(pinned_runner=runner, status=AgentRunStatus.QUEUED, pin_strict=True)
        .values_list("pk", flat=True)
    )
    for run_id in strict_ids:
        finalize_agent_run(
            run_id,
            AgentRunStatus.FAILED,
            expected_status=AgentRunStatus.QUEUED,
            updates={
                "error_code": ASSIGNED_RUNNER_REMOVED,
                "error": f"{runner.machine_display_name} was removed from this project, so this run was not started.",
            },
        )
    issue_ids = list(Issue.all_objects.filter(assigned_runner=runner).values_list("pk", flat=True))
    if issue_ids:
        Issue.all_objects.filter(pk__in=issue_ids).update(assigned_runner=None)
        _record_assignment_activity(
            issue_ids,
            old_runner=runner,
            comment=f"{runner.machine_display_name} was removed, so this issue now runs on its pod.",
        )


def rehome_assigned_work(runner: Runner, new_pod) -> None:
    """A runner moved to ``new_pod``: move its assigned issues and its queued
    strict runs with it, in the caller's transaction (§7)."""
    from pi_dash.db.models.issue import Issue

    Issue.all_objects.filter(assigned_runner=runner).update(assigned_pod=new_pod)
    AgentRun.objects.filter(pinned_runner=runner, status=AgentRunStatus.QUEUED, pin_strict=True).update(
        pod=new_pod
    )


def repin_queued_runs_for_issue(issue) -> None:
    """The issue's assignment changed: re-point its queued runs (§7).

    With a runner, every queued run of the issue becomes strictly pinned to it
    in its pod; without one, strict pins are released to the issue's pod.
    """
    runner = live_assigned_runner(issue)
    queued = AgentRun.objects.filter(work_item=issue, status=AgentRunStatus.QUEUED)
    if runner is not None:
        queued.update(pinned_runner=runner, pin_strict=True, pod=runner.pod)
        pod_id = runner.pod_id
    else:
        pod_id = issue.assigned_pod_id
        updates = {"pinned_runner": None, "pin_strict": False}
        if pod_id is not None:
            updates["pod_id"] = pod_id
        queued.filter(pin_strict=True).update(**updates)
    if pod_id is not None:
        from pi_dash.runner.services.matcher import drain_pod_by_id

        transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))
