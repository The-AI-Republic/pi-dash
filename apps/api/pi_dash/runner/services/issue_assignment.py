# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared execution-target rules for issues.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §6 (revision
4.0) — ``normalize_issue_executor`` below.

Dedicated-runner design (``.ai_design/dedicated_runner_assignment/design.md``,
HA §6-§14): read-side helpers (``queue_depth``, ``is_busy``, ``assignable``,
``assigned_runner_detail``, ``dispatch_wait``) used by both the web-app and v1
serializers and the runner list. Write-side validation (``validate_execution_target``,
``apply_execution_target``) lands with dispatch so the write path cannot reach
main before dispatch honours it (H-5).
"""

from __future__ import annotations

from datetime import timedelta

from django.conf import settings
from django.utils import timezone
from rest_framework import serializers

from pi_dash.core.agent_execution import AgentExecutorKind, normalize_executor_write


def normalize_issue_executor(attrs: dict, *, instance) -> None:
    """Translate and validate ``attrs["agent_executor"]`` in place.

    ``None`` clears the override (inherit the project default). A change of
    executor under a live run is refused, as before: ``AgentRun.executor_kind``
    is snapshotted at creation, so switching under a live run would silently
    apply only to the next dispatch. Raises ``serializers.ValidationError``.
    """
    if "agent_executor" not in attrs:
        return
    executor = attrs["agent_executor"]
    if executor is not None and executor not in AgentExecutorKind.values:
        raise serializers.ValidationError({"agent_executor": "unknown agent executor"})
    attrs["agent_executor"] = normalize_executor_write(executor)
    if (
        instance is not None
        and str(attrs["agent_executor"]) != str(instance.agent_executor)
        and instance.has_active_run
    ):
        raise serializers.ValidationError(
            {"agent_executor": "cannot change the execution target while the issue has an active run"}
        )


# ---------------------------------------------------------------------------
# Read-side helpers (design §7.4, §10.1, §11.1, §14.1, §14.3)
# ---------------------------------------------------------------------------


def queue_depth(runner) -> int:
    """Number of QUEUED runs pinned to ``runner`` (design §10.1)."""
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    return AgentRun.objects.filter(pinned_runner=runner, status=AgentRunStatus.QUEUED).count()


def is_busy(runner) -> bool:
    """True when ``runner`` has a run in BUSY_STATUSES (design §10.1)."""
    from pi_dash.runner.models import AgentRun
    from pi_dash.runner.services.matcher import BUSY_STATUSES

    return AgentRun.objects.filter(runner=runner, status__in=BUSY_STATUSES).exists()


def runner_is_offline(runner, *, now=None) -> bool:
    """Offline per design §11.1 (status or heartbeat freshness)."""
    from pi_dash.runner.models import RunnerStatus
    from pi_dash.runner.services.matcher import HEARTBEAT_GRACE

    now = now or timezone.now()
    if runner.status != RunnerStatus.ONLINE:
        return True
    if runner.last_heartbeat_at is None:
        return True
    return runner.last_heartbeat_at < now - HEARTBEAT_GRACE


def runner_paused_reason(runner):
    """Why ``runner`` is paused, or ``None`` when it is not paused (design §11.1, §23.5).

    Returns ``dispatch_paused`` for an operator pause, ``owner_paused`` for an
    engine paused by its owner, else the machine's ``engine_auto_pause_reason``.
    Paused wins over offline in ``dispatch_wait``/``dedicated_run_refusal`` because
    a person must act on it.
    """
    if runner.paused_at is not None:
        return "dispatch_paused"
    machine = getattr(runner, "dev_machine", None)
    # ``dev_machine`` may be a deferred attribute; avoid N+1 by reading the cached
    # relation only when the FK is set. Callers that need accuracy pass a runner
    # with ``select_related("dev_machine")`` or the FK already loaded.
    if machine is not None and getattr(runner, "is_engine", False):
        if getattr(machine, "engine_paused_by_owner", False):
            return "owner_paused"
        reason = getattr(machine, "engine_auto_pause_reason", "") or ""
        if reason:
            return reason
    return None


def assigned_runner_detail(runner):
    """The ``assigned_runner_detail`` object (design §14.1) or ``None``.

    Shown to everyone who can see the issue (H-6), even though the runner
    list itself is owner-only.
    """
    if runner is None:
        return None
    return {
        "id": str(runner.id),
        "name": runner.name,
        "status": runner.status,
        "pod": str(runner.pod_id) if runner.pod_id else None,
        "owner": str(runner.owner_id) if runner.owner_id else None,
        "owner_display_name": getattr(runner.owner, "display_name", "") or "",
        "host_label": runner.host_label or "",
        "last_heartbeat_at": runner.last_heartbeat_at.isoformat() if runner.last_heartbeat_at else None,
        "is_busy": is_busy(runner),
        "queue_depth": queue_depth(runner),
    }


def assignable_for_runner(runner, *, user, issue=None):
    """Evaluate E1-E8 (sans E6) for ``runner`` as a dedicated choice (design §7.1).

    Returns ``(assignable, code)`` where ``code`` is ``""`` when assignable,
    else the first failing E-code in E1-E8 order. Read-side only: the write
    path (``validate_execution_target``) shares these rules but raises.
    """
    from pi_dash.runner.models import AgentRunStatus, RunnerProvisioning, RunnerStatus
    from pi_dash.runner.services.permissions import can_use_runner

    # E1: the requester can use the runner (today: owns it).
    if not can_use_runner(user, runner):
        return False, "runner_not_found"
    # E2: the runner's pod is in the issue's project (or, for "new", the
    # project passed via the issue context). When ``issue`` is None the
    # project check is the caller's responsibility (runner list is already
    # project-scoped) and only E1 applies.
    if issue is not None:
        if runner.pod_id is None or str(runner.pod.project_id) != str(issue.project_id):
            return False, "runner_wrong_project"
    # E3: a request that also names a pod must name the runner's pod.
    # The list read never names a pod alongside the runner, so E3 is
    # vacuously satisfied here; the write path enforces it.
    # E4: not revoked.
    if runner.status == RunnerStatus.REVOKED or runner.revoked_at is not None:
        return False, "runner_revoked"
    # E5: lifted for engine runners (§23.1) — a desktop_bundled runner is now
    # choosable, so no check here.
    # E7: the runner's owner is the issue's creator or a current assignee.
    # For "new" the requester is the future creator, so the caller passes an
    # issue-like object with ``created_by_id == user.id`` (or ``issue=None``
    # with a sentinel; see ``assignable_runners``).
    if issue is not None:
        from pi_dash.db.models.issue import IssueAssignee

        people = set()
        if getattr(issue, "created_by_id", None) is not None:
            people.add(issue.created_by_id)
        people.update(
            IssueAssignee.objects.filter(issue=issue).values_list("assignee_id", flat=True)
        )
        people.discard(None)
        if runner.owner_id not in people:
            return False, "runner_owner_not_on_issue"
    # E8: no run the runner holds; QUEUED and PAUSED_AWAITING_INPUT pass.
    if issue is not None:
        from pi_dash.runner.models import AgentRun
        from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES

        blocking = [
            s for s in NON_TERMINAL_STATUSES
            if s not in (AgentRunStatus.QUEUED, AgentRunStatus.PAUSED_AWAITING_INPUT)
        ]
        if AgentRun.objects.filter(work_item=issue, status__in=blocking).exists():
            return False, "active_run_in_progress"
    _ = RunnerProvisioning  # E5 lifted; kept for reference
    return True, ""


def assignable_runners(*, project, user, issue=None):
    """Runners visible to ``user`` in ``project`` (design §7.4).

    Starts from the same visibility gate as the runner list so the picker
    and the CLI cannot drift. ``issue=None`` means "new" (the requester is
    the future creator) — E7 is then evaluated with the requester as creator.
    """
    from pi_dash.runner.models import Runner
    from pi_dash.runner.services.permissions import runner_visible_to_user_q

    qs = (
        Runner.objects.filter(pod__project=project)
        .filter(runner_visible_to_user_q(user))
        .select_related("pod__project", "dev_machine", "owner")
        .order_by("-updated_at")
    )
    if issue is None:
        # "new": E7 passes for any runner the requester owns, since the
        # requester will be the creator. The per-row evaluation still checks
        # E1/E4; E7 is satisfied by construction.
        return qs
    return qs


def _iso(value):
    return value.isoformat() if value else None


def build_dispatch_wait(run, *, viewer=None):
    """The ``dispatch_wait`` object for a QUEUED pinned run (design §14.3), else ``None``.

    ``viewer`` is the requesting user, used for the ``releasable`` permission
    check on locality pins. Dedicated pins are always ``releasable: false``
    (H-13). ``pause_reason`` is set only with ``runner_paused``; ``expires_at``
    is ``queued_at + DEDICATED_RUN_QUEUED_MAX_AGE_SECS`` for dedicated pins,
    else ``null``.
    """
    from pi_dash.runner.models import AgentRunStatus, PinKind
    from pi_dash.runner.services.permissions import can_use_runner

    if run is None or run.status != AgentRunStatus.QUEUED or run.pinned_runner_id is None:
        return None
    runner = run.pinned_runner
    if runner is None:
        # Pinned runner deleted between read and serialize; degrade to
        # dispatching rather than raising.
        runner_name = None
        runner_last_seen = None
        reason = "dispatching"
        pause_reason = None
        runs_ahead = 0
    else:
        runner_name = runner.name
        # Paused wins over offline (§11.1).
        pause_reason = runner_paused_reason(runner)
        runs_ahead = AgentRunCountAhead(run)
        if pause_reason is not None:
            reason = "runner_paused"
        elif runner_is_offline(runner):
            reason = "runner_offline"
        elif is_busy(runner) or runs_ahead > 0:
            reason = "runner_busy"
        else:
            # Online, not paused, not busy: dispatching unless the usage rule
            # excludes this run (stranded, §9.4/§12.1).
            from pi_dash.runner.models import HUMAN_TRIGGERS
            from pi_dash.runner.services.dispatch_rules import runner_may_take

            issue = run.work_item if run.work_item_id else None
            human = run.trigger in HUMAN_TRIGGERS if run.trigger else True
            if issue is not None and not runner_may_take(
                runner, created_by_id=run.created_by_id, human=human, issue=issue
            ):
                reason = "runner_not_permitted"
            else:
                reason = "dispatching"
        runner_last_seen = runner.last_heartbeat_at

    if run.pin_kind == PinKind.DEDICATED:
        releasable = False
        expires_at = run.created_at + timedelta(seconds=settings.DEDICATED_RUN_QUEUED_MAX_AGE_SECS) if run.created_at else None
    else:
        # Locality pin: follows the release endpoint's permission check
        # (issue creator / assignees or runner owner, mirroring
        # ``_can_cancel_run``'s locality arm). The dedicated refusal (H-13)
        # is handled above; this branch is the pre-H-13 rule.
        releasable = False
        if viewer is not None and run.work_item_id is not None:
            try:
                from pi_dash.runner.views.runs import _can_cancel_run

                releasable = bool(_can_cancel_run(viewer, run))
            except Exception:
                releasable = False
        expires_at = None

    return {
        "reason": reason,
        "pin_kind": run.pin_kind,
        "runner": str(run.pinned_runner_id) if run.pinned_runner_id else None,
        "runner_name": runner_name,
        "queued_at": _iso(run.created_at),
        "runner_last_seen_at": _iso(runner_last_seen),
        "runs_ahead": runs_ahead,
        "pause_reason": pause_reason if reason == "runner_paused" else None,
        "expires_at": _iso(expires_at) if expires_at else None,
        "releasable": releasable,
    }


def AgentRunCountAhead(run) -> int:
    """Older QUEUED runs pinned to the same runner (read-side runs_ahead)."""
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    if run.created_at is None or run.pinned_runner_id is None:
        return 0
    return AgentRun.objects.filter(
        pinned_runner_id=run.pinned_runner_id,
        status=AgentRunStatus.QUEUED,
        created_at__lt=run.created_at,
    ).exclude(pk=run.pk).count()
