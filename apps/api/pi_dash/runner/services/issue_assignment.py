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


def _check_assignment_code(runner, *, user, issue, pod=None, project_id=None):
    """Shared E1-E8 (sans E6) evaluation for one dedicated choice (design §7.1).

    Returns ``(code, detail)`` — ``code`` is ``""`` when the choice is
    assignable, else the first failing E-code in E1-E8 order, with the
    human-readable ``detail`` for the refusal. ``issue=None`` means "new":
    E7 is satisfied by construction (the requester is the future creator)
    and E2 reads ``project_id`` (the create surface passes the context
    project). ``pod`` is the requested execution pod, or ``None`` when the
    request names no pod.
    """
    from pi_dash.runner.models import AgentRunStatus, RunnerStatus
    from pi_dash.runner.services.permissions import can_use_runner

    # E1: the requester can use the runner (today: owns it).
    if not can_use_runner(user, runner):
        return "runner_not_found", "runner not found"
    # E2: the runner's pod is in the issue's project.
    if issue is not None:
        if runner.pod_id is None or str(runner.pod.project_id) != str(issue.project_id):
            return "runner_wrong_project", "runner is in a different project"
    elif project_id is not None:
        if runner.pod_id is None or str(runner.pod.project_id) != str(project_id):
            return "runner_wrong_project", "runner is in a different project"
    elif pod is not None:
        if str(runner.pod.project_id) != str(pod.project_id):
            return "runner_wrong_project", "runner is in a different project"
    # E3: a request that also names a pod must name the runner's pod.
    if pod is not None and runner.pod_id is not None and str(pod.id) != str(runner.pod_id):
        return "runner_pod_mismatch", "runner is in a different pod"
    if pod is not None and runner.pod_id is None:
        return "runner_pod_mismatch", "runner is in a different pod"
    # E4: not revoked.
    if runner.status == RunnerStatus.REVOKED or runner.revoked_at is not None:
        return "runner_revoked", "runner has been revoked"
    # E5: lifted for engine runners (§23.1) — a desktop_bundled runner is now
    # choosable, so no check here.
    # E7: the runner's owner is the issue's creator or a current assignee.
    if issue is not None and getattr(issue, "pk", None) is not None:
        from pi_dash.db.models.issue import IssueAssignee

        people = set()
        if getattr(issue, "created_by_id", None) is not None:
            people.add(issue.created_by_id)
        people.update(
            IssueAssignee.objects.filter(issue=issue).values_list("assignee_id", flat=True)
        )
        people.discard(None)
        if runner.owner_id not in people:
            return (
                "runner_owner_not_on_issue",
                "the runner's owner is not on this issue",
            )
    # E8: no run the runner holds; QUEUED and PAUSED_AWAITING_INPUT pass.
    if issue is not None and getattr(issue, "pk", None) is not None:
        from pi_dash.runner.models import AgentRun
        from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES

        blocking = [
            s for s in NON_TERMINAL_STATUSES
            if s not in (AgentRunStatus.QUEUED, AgentRunStatus.PAUSED_AWAITING_INPUT)
        ]
        if AgentRun.objects.filter(work_item=issue, status__in=blocking).exists():
            return (
                "active_run_in_progress",
                "the issue has an active run; changes take effect on the next run",
            )
    return "", ""


def assignable_for_runner(runner, *, user, issue=None):
    """Evaluate E1-E8 (sans E6) for ``runner`` as a dedicated choice (design §7.1).

    Returns ``(assignable, code)`` where ``code`` is ``""`` when assignable,
    else the first failing E-code in E1-E8 order. Read-side only: the write
    path (``validate_execution_target``) shares these rules but raises.
    """
    code, _detail = _check_assignment_code(runner, user=user, issue=issue)
    return (code == "", code)


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
        expires_at = (
            run.created_at + timedelta(seconds=settings.DEDICATED_RUN_QUEUED_MAX_AGE_SECS)
            if run.created_at
            else None
        )
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
    """Older QUEUED runs pinned to the same runner (read-side runs_ahead).

    Under the §10.2 alternation a dedicated run may also wait behind one
    shared run (rank 1 beats rank 2 when the last run was dedicated) and
    behind newer locality pins (rank 0 always); those count too.
    """
    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind

    if run.created_at is None or run.pinned_runner_id is None:
        return 0
    ahead = (
        AgentRun.objects.filter(
            pinned_runner_id=run.pinned_runner_id,
            status=AgentRunStatus.QUEUED,
            created_at__lt=run.created_at,
        )
        .exclude(pk=run.pk)
        .count()
    )
    if run.pin_kind == PinKind.DEDICATED:
        from pi_dash.runner.services.matcher import _last_served_dedicated

        runner = run.pinned_runner
        if runner is not None and _last_served_dedicated(runner):
            # Locality pins outrank a dedicated run whatever their age.
            ahead += (
                AgentRun.objects.filter(
                    pinned_runner=run.pinned_runner,
                    status=AgentRunStatus.QUEUED,
                    pin_kind=PinKind.LOCALITY,
                    created_at__gte=run.created_at,
                )
                .exclude(pk=run.pk)
                .count()
            )
            # One shared run is served before the next dedicated (§10.2).
            if AgentRun.objects.filter(
                pod=run.pod,
                status=AgentRunStatus.QUEUED,
                pinned_runner__isnull=True,
            ).exists():
                ahead += 1
    return ahead


# ---------------------------------------------------------------------------
# Pin resolver (design §9.1)
# ---------------------------------------------------------------------------


def locality_runner_for(parent, target_pod, *, creator, trigger, issue):
    """Locality pin: the parent's runner while it takes pod work now.

    Keeps every rule ``_pinned_runner_for`` had on main: online, heartbeat-
    fresh, not paused, same pod, ``locality_pin_allowed`` (an engine only
    while ``engine_takes_pod_work``), and for an engine ``runner_may_take``
    for the new run's creator and trigger.
    """
    from pi_dash.runner.models import HUMAN_TRIGGERS, RunnerStatus
    from pi_dash.runner.services.dispatch_rules import locality_pin_allowed, runner_may_take
    from pi_dash.runner.services.matcher import HEARTBEAT_GRACE

    if parent is None or parent.runner_id is None:
        return None
    runner = parent.runner
    if runner is None:
        return None
    if runner.status in (RunnerStatus.REVOKED, RunnerStatus.OFFLINE):
        return None
    if runner.paused_at is not None:
        return None
    if runner.last_heartbeat_at is None or runner.last_heartbeat_at < timezone.now() - HEARTBEAT_GRACE:
        return None
    if target_pod is not None and runner.pod_id != target_pod.id:
        return None
    if not locality_pin_allowed(runner):
        return None
    if runner.is_engine and not runner_may_take(
        runner, created_by_id=getattr(creator, "id", None), human=trigger in HUMAN_TRIGGERS, issue=issue
    ):
        return None
    return runner


def clear_dedicated_runner(issue, *, reason, actor, decided_by):
    """Clear ``Issue.assigned_runner`` and record why (design §13).

    Always clears; raises only when the issue row or activity write fails.
    Every clear writes an activity entry naming the runner and one system
    comment naming the runner and the reason (§13.1/§14.7), attributed to
    ``actor`` (the system actor for system-initiated clears).
    ``assigned_pod`` is left alone.

    ``decided_by`` is the moment the dedicated assignment was bypassed
    deliberately (H-12); ``None`` for system-initiated clears.
    """
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueActivity, IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user

    runner = getattr(issue, "assigned_runner", None)
    runner_name = runner.name if runner is not None else "the dedicated runner"
    runner_id = str(runner.id) if runner is not None else None
    issue.assigned_runner = None
    issue.save(update_fields=["assigned_runner", "updated_at"])
    if actor is None:
        actor = get_agent_system_user()
    body = format_html(
        "<p>The dedicated runner assignment to <strong>{}</strong> was cleared: <strong>{}</strong>.</p>",
        runner_name,
        reason,
    )
    IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=actor,
        comment_html=body,
        speaker_type=IssueComment.SpeakerType.SYSTEM,
    )
    IssueActivity.objects.create(
        issue=issue,
        actor=actor,
        verb="updated",
        old_value=runner_name,
        new_value="",
        field="assigned_runner",
        project_id=issue.project_id,
        workspace_id=issue.workspace_id,
        comment=f"cleared the dedicated runner {runner_name}: {reason}",
        old_identifier=runner_id,
        new_identifier=None,
        epoch=1,
    )


def resolve_run_pin(issue, *, parent, pod, fresh_session, creator, trigger, pod_once=False):
    """Return ``(runner, pin_kind)`` for a run created on ``issue`` (design §9.1).

    Dedicated-first: ``issue.assigned_runner`` wins unless ``pod_once``
    (§23.3) deliberately bypasses it. A revoked or pod-mismatched
    dedicated runner is self-healed (cleared with ``stale_at_dispatch``)
    and dispatch falls through to locality. Offline/paused/busy dedicated
    runners stay pinned — a person's run is refused before it reaches
    here (§11), so only automatic runs arrive pinned to a runner that is
    not taking work.
    """
    from pi_dash.runner.models import PinKind, RunnerStatus

    assigned = getattr(issue, "assigned_runner", None)
    # ``resolve_run_pin`` requires a resolvable assignment, not test fakes
    # with ``assigned_runner_id=None`` coerced to an object. Only the real
    # FK traversal is authoritative; test doubles that expose the attribute
    # as a mock would otherwise be mistaken for an assignment.
    if assigned is not None and getattr(assigned, "pk", None) is None:
        assigned = None
    if assigned is not None and not pod_once:
        if assigned.status == RunnerStatus.REVOKED or str(assigned.pod_id) != str(pod.id):
            clear_dedicated_runner(issue, reason="stale_at_dispatch", actor=None, decided_by=None)
        else:
            return assigned, PinKind.DEDICATED
    if fresh_session or parent is None or pod_once:
        return None, PinKind.LOCALITY
    return (
        locality_runner_for(parent, pod, creator=creator, trigger=trigger, issue=issue),
        PinKind.LOCALITY,
    )


# ---------------------------------------------------------------------------
# Write path: validation + apply (design §7.1-§7.3, §8, §13.2)
# ---------------------------------------------------------------------------


def validate_execution_target(attrs, *, issue=None, project_id=None, user=None):
    """Validate the requested execution target in place (design §7.1).

    ``attrs`` is the serializer's validated data. When ``assigned_runner``
    (or the ``_id`` spelling) is present, E1-E8 are evaluated and the first
    failure raises ``serializers.ValidationError`` keyed on the field the
    caller posted, carrying the E-code as the error ``code``. Skips runner
    checks when the request names no runner — except E8: clearing the
    runner under an ASSIGNED/RUNNING run is refused with
    ``active_run_in_progress`` (§13.2). ``project_id`` carries E2 on create
    (when there is no issue yet).
    """
    if user is None:
        from crum import get_current_user

        user = get_current_user()
    runner_key = "assigned_runner" if "assigned_runner" in attrs else (
        "assigned_runner_id" if "assigned_runner_id" in attrs else None
    )
    if runner_key is None:
        return
    runner = attrs.get(runner_key)
    if runner is None:
        # A clear is a runner change: E8 applies (§13.2). The no-op case
        # (already unassigned) passes by construction.
        if issue is not None and getattr(issue, "pk", None) is not None:
            from pi_dash.runner.models import AgentRun, AgentRunStatus
            from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES

            blocking = [
                s for s in NON_TERMINAL_STATUSES
                if s not in (AgentRunStatus.QUEUED, AgentRunStatus.PAUSED_AWAITING_INPUT)
            ]
            if AgentRun.objects.filter(work_item=issue, status__in=blocking).exists():
                raise serializers.ValidationError(
                    {
                        runner_key: serializers.ErrorDetail(
                            "the issue has an active run; changes take effect on the next run",
                            code="active_run_in_progress",
                        )
                    }
                )
        return
    # No-op resend (§7.2): re-sending the current runner is never rejected,
    # even after eligibility drifted. Read-side evaluation
    # (assignable_for) is unaffected — this is the write path only.
    if issue is not None and getattr(issue, "assigned_runner_id", None) is not None:
        if str(runner.pk) == str(issue.assigned_runner_id):
            return
    pod = attrs.get("assigned_pod") or attrs.get("assigned_pod_id")
    if pod is not None and not hasattr(pod, "project_id"):
        from pi_dash.runner.models import Pod

        pod = Pod.objects.filter(pk=pod).first()
    code, detail = _check_assignment_code(runner, user=user, issue=issue, pod=pod, project_id=project_id)
    if code:
        raise serializers.ValidationError({runner_key: serializers.ErrorDetail(detail, code=code)})


def _repoint_queued_run(run, *, new_runner, actor, decided_by):
    """Re-point one QUEUED run at its new dedicated pin (§13.2)."""
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user
    from pi_dash.runner.models import PinKind

    old = run.pinned_runner
    old_name = old.name if old is not None else "the pod queue"
    if actor is None:
        actor = get_agent_system_user()
    run.pinned_runner = new_runner
    run.pin_kind = PinKind.DEDICATED if new_runner is not None else PinKind.LOCALITY
    run.save(update_fields=["pinned_runner", "pin_kind"])
    # §13.2: a re-pointed run starts a fresh session on its new target —
    # the parent's thread id is stale. Drain the run's pod on commit so
    # the new pin is served promptly.
    parent = getattr(run, "parent_run", None)
    if parent is not None and parent.thread_id:
        parent.thread_id = ""
        parent.save(update_fields=["thread_id"])
    if run.pod_id is not None:
        from django.db import transaction

        pod_id = run.pod_id

        def _drain(pod_id=pod_id):
            from pi_dash.runner.services.matcher import drain_pod_by_id

            drain_pod_by_id(pod_id)

        transaction.on_commit(_drain)
    body = format_html(
        "<p>Your run was re-pointed from <strong>{}</strong> to <strong>{}</strong>.</p>",
        old_name,
        new_runner.name if new_runner is not None else "the pod queue",
    )
    issue = run.work_item
    if issue is not None:
        IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=actor,
            comment_html=body,
            speaker_type=IssueComment.SpeakerType.SYSTEM,
        )


def apply_execution_target(issue, *, new_runner, actor, decided_by, project=None):
    """Persist the chosen execution target (design §13.2).

    Saves ``assigned_runner``, re-points the issue's QUEUED run (if any),
    and writes an activity row. Call only after
    ``validate_execution_target`` has passed. ``decided_by`` is the moment
    the choice was made; the caller stamps it.

    One transaction: the issue row is locked first, then the run row (the
    issue -> run order ``complete_project_move_handoff`` uses). If the run
    was assigned between the serializer's read and the lock, the row lock
    serialization makes the QUEUED re-check fail-closed: no re-point.
    """
    from django.db import transaction
    from django.utils import timezone as dj_timezone

    from pi_dash.db.models.issue import Issue, IssueActivity
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    decided_at = decided_by or dj_timezone.now()
    with transaction.atomic():
        # Lock the issue row first (issue -> run order, §13.2).
        Issue.objects.select_for_update(of=("self",)).filter(pk=issue.pk).first()
        issue.assigned_runner = new_runner
        if new_runner is not None:
            issue.assigned_pod = new_runner.pod
        issue.save(update_fields=["assigned_runner", "assigned_pod", "updated_at"])
        queued = (
            AgentRun.objects.select_for_update()
            .filter(work_item=issue, status=AgentRunStatus.QUEUED)
            .order_by("created_at")
            .first()
        )
        if queued is not None:
            _repoint_queued_run(queued, new_runner=new_runner, actor=actor, decided_by=decided_at)
        old_name = ""
        new_name = new_runner.name if new_runner is not None else ""
        IssueActivity.objects.create(
            issue=issue,
            actor=actor,
            verb="updated",
            old_value=old_name,
            new_value=new_name,
            field="assigned_runner",
            project_id=issue.project_id,
            workspace_id=issue.workspace_id,
            comment="updated the dedicated runner to",
            old_identifier=None,
            new_identifier=str(new_runner.id) if new_runner is not None else None,
            epoch=1,
        )


# ---------------------------------------------------------------------------
# Run-time refusal + lifecycle (design §11, §12, §13)
# ---------------------------------------------------------------------------


def dedicated_run_refusal(issue, *, human, user, pod_once=False):
    """Pre-creation refusal for a person's run (§11, §23.3).

    Returns ``(code, detail)`` — ``assigned_runner_not_owner``,
    ``assigned_runner_offline`` or ``assigned_runner_paused`` — or
    ``(None, None)`` when the run may proceed. Only a person's run is
    refused; an automatic run is created and waits (§11.3). Busy is never
    a refusal. Skipped entirely on a ``pod_once`` request (§23.3): the
    run goes to the pod instead.

    Order (§11.2): the H-10 usage-rule check first, so a teammate
    excluded by an engine's usage rule gets ``assigned_runner_not_owner``
    whatever the engine's state; the availability check second.
    """
    if not human:
        return None, None
    if pod_once:
        return None, None
    runner = getattr(issue, "assigned_runner", None)
    if runner is None:
        return None, None
    if runner.revoked_at is not None:
        return None, None  # E4 already failed at assignment; nothing to refuse here
    # H-10 first: the engine usage rule (and, via runner_may_take, the
    # own-agent rule) is checked before availability so an excluded
    # teammate is refused with assigned_runner_not_owner even when the
    # runner is also offline/paused (§23.3).
    from pi_dash.runner.services.dispatch_rules import runner_may_take

    user_id = getattr(user, "id", None) if user is not None else None
    if not runner_may_take(runner, created_by_id=user_id, human=True, issue=issue):
        return "assigned_runner_not_owner", "the dedicated runner's usage rule excludes this run"
    pause = runner_paused_reason(runner)
    if pause is not None:
        return "assigned_runner_paused", pause
    if runner_is_offline(runner):
        return "assigned_runner_offline", "dedicated runner is offline"
    return None, None


def refusal_pause_phrase(pause_reason) -> str:
    """Plain-words pause phrase for refusal ``error`` text (§11.2)."""
    if pause_reason == "dispatch_paused":
        return "is paused by its owner"
    if pause_reason == "owner_paused":
        return "is paused by its owner"
    if pause_reason == "wallet_empty":
        return "is paused: the wallet is empty"
    if pause_reason == "engine_sign_in_required":
        return "is paused: the engine's sign-in expired"
    if pause_reason == "gateway_session_revoked":
        return "is paused: the gateway session was revoked"
    if pause_reason == "profile_unavailable":
        return "is paused: the engine profile is unavailable"
    if pause_reason == "engine_disabled":
        return "is paused: the engine is disabled"
    return "is paused"


def dedicated_refusal_body(issue, *, code):
    """The full §11.2 refusal body for ``code`` on ``issue``.

    Shape: ``{error, reason, pause_reason, runner, runner_name,
    runner_last_seen_at, pod, pod_name, options}``. ``options`` is
    ``["pod_once", "change_runner"]`` for availability refusals and
    ``["run_on_my_runner", "pod_once"]`` for ``assigned_runner_not_owner``.
    """
    runner = getattr(issue, "assigned_runner", None)
    pod = getattr(issue, "assigned_pod", None)
    pause_reason = None
    if runner is not None and code == "assigned_runner_paused":
        pause_reason = runner_paused_reason(runner) or "dispatch_paused"
    if code == "assigned_runner_not_owner":
        options = ["run_on_my_runner", "pod_once"]
    else:
        options = ["pod_once", "change_runner"]
    if runner is None:
        error = "the dedicated runner is unavailable"
    elif code == "assigned_runner_not_owner":
        error = f"{runner.name} cannot take this run (not the runner's owner)"
    elif code == "assigned_runner_paused":
        error = f"{runner.name} {refusal_pause_phrase(pause_reason)}"
    elif code == "assigned_runner_offline":
        seen = runner.last_heartbeat_at.isoformat() if runner.last_heartbeat_at else "unknown"
        error = f"{runner.name} is offline (last seen {seen})"
    else:
        error = "the dedicated runner is unavailable"
    return {
        "error": error,
        "reason": code,
        "pause_reason": pause_reason,
        "runner": str(runner.id) if runner is not None else None,
        "runner_name": runner.name if runner is not None else None,
        "runner_last_seen_at": runner.last_heartbeat_at.isoformat()
        if runner is not None and runner.last_heartbeat_at
        else None,
        "pod": str(pod.id) if pod is not None else None,
        "pod_name": pod.name if pod is not None else None,
        "options": options,
    }


def pod_once_auth_code(issue, *, user):
    """The ``pod_once`` authorization refusal for ``user`` on ``issue`` (§23.3).

    Returns ``pod_once_not_applicable`` when the issue has no dedicated
    runner, ``pod_once_not_allowed`` when the user is neither the issue's
    creator nor a current assignee, else ``None``. ``no_eligible_runner_in_pod``
    is a preflight outcome, not an auth code, and is returned by the
    dispatch paths after the pod-branch preflight.
    """
    runner = getattr(issue, "assigned_runner", None)
    if runner is None:
        return "pod_once_not_applicable"
    user_id = getattr(user, "id", None) if user is not None else None
    if user_id is None:
        return "pod_once_not_allowed"
    if issue.created_by_id is not None and issue.created_by_id == user_id:
        return None
    from pi_dash.db.models.issue import IssueAssignee

    if IssueAssignee.objects.filter(issue=issue, assignee_id=user_id).exists():
        return None
    return "pod_once_not_allowed"


def refusal_detail(short_code, new_runner=None):
    """The H-11 recovery payload in a run's ``error`` field (design §11.2)."""
    detail = (
        "Automatic run failed: dedicated runner unavailable."
    )
    if new_runner is not None:
        detail += f" Your previous dedicated runner was {new_runner.name}."
    detail += " Action: unassign the runner or run on the pod."
    return detail


def _stranded_key(run):
    return (run.created_by_id, run.trigger in {"tick", "state_transition"})


def stranded_queued(run, issue):
    """True when a dedicated run's runner excludes it under the usage rule (§12.1)."""
    from pi_dash.runner.models import HUMAN_TRIGGERS
    from pi_dash.runner.services.dispatch_rules import runner_may_take

    runner = run.pinned_runner
    if runner is None or issue is None:
        return False
    human = run.trigger in HUMAN_TRIGGERS if run.trigger else True
    return not runner_may_take(
        runner, created_by_id=run.created_by_id, human=human, issue=issue
    )


def dedicated_unavailable_error(runner, *, pod_name="the pod") -> str:
    """The §11.3 failure text for a dedicated 12h bound expiry."""
    pause = runner_paused_reason(runner)
    if pause is not None:
        detail = f"it {refusal_pause_phrase(pause)}"
    else:
        seen = runner.last_heartbeat_at.isoformat() if runner.last_heartbeat_at else "unknown"
        detail = f"it is offline (last seen {seen})"
    return (
        f"Runner {runner.name} did not take this run within 12 hours: {detail}. "
        f"Automatic runs on this issue are stopped. "
        f"The issue stays assigned to {runner.name}. "
        "When the runner is back, use Run or Re-tick. "
        f"To run it elsewhere, use Run on any runner in {pod_name} once, "
        "or change the issue's runner."
    )


def fail_dedicated_runs() -> int:
    """Fail dedicated runs whose runner is still unavailable after 12h (§11.3).

    Every dedicated QUEUED run older than
    ``DEDICATED_RUN_QUEUED_MAX_AGE_SECS`` whose pinned runner is
    unavailable now (§11.1) fails with ``dedicated_runner_unavailable``
    and ``done_payload.stop_ticking`` so the clock stops (§11.3). The
    unavailability is re-checked under the row lock (as
    ``runner.tasks._release_pins`` does) so a runner that came back in
    the meantime keeps its run. A run behind a busy online runner is not
    failed: it is moving.
    """
    from datetime import timedelta as td

    from django.conf import settings as dj_settings
    from django.db import transaction

    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    cutoff = timezone.now() - td(seconds=dj_settings.DEDICATED_RUN_QUEUED_MAX_AGE_SECS)
    stale = list(
        AgentRun.objects.filter(
            pin_kind=PinKind.DEDICATED,
            status=AgentRunStatus.QUEUED,
            created_at__lt=cutoff,
            pinned_runner__isnull=False,
        ).values_list("id", flat=True)[:500]
    )
    failed = 0
    for run_id in stale:
        with transaction.atomic():
            run = (
                AgentRun.objects.select_for_update(of=("self",))
                .select_related("pinned_runner", "pod", "work_item")
                .filter(pk=run_id, status=AgentRunStatus.QUEUED, pin_kind=PinKind.DEDICATED)
                .first()
            )
            if run is None:
                continue
            runner = run.pinned_runner
            if runner is None:
                continue
            # Locked re-check: a runner that came back keeps its run.
            runner.refresh_from_db(fields=["status", "paused_at", "last_heartbeat_at"])
            if runner_paused_reason(runner) is None and not runner_is_offline(runner):
                continue
            pod_name = run.pod.name if run.pod_id is not None and run.pod else "the pod"
            # finalize re-uses the row lock we hold; expected_status keeps
            # it first-writer-wins if the candidate moved since the list.
            if finalize_agent_run(
                run_id,
                AgentRunStatus.FAILED,
                expected_status=AgentRunStatus.QUEUED,
                updates={
                    "error_code": "dedicated_runner_unavailable",
                    "error": dedicated_unavailable_error(runner, pod_name=pod_name),
                    "done_payload": {"stop_ticking": True},
                },
            ):
                failed += 1
    return failed


def _lost_runner_revoke_error(runner_name) -> str:
    """Run ``error`` text for a queued dedicated run lost with its runner."""
    return (
        f"Runner {runner_name} was revoked or deleted, so this run cannot start. "
        "The issue is back in the pod's routing; choose a new runner or keep the pod."
    )


def apply_lost_runner_policy(issue, *, reason):
    """Apply the project's lost-runner handling to one issue (§13.1).

    Called inside the revoking transaction, after the assignment has been
    read; clears the assignment itself. Applies whether or not the issue
    has a queued dedicated run.

    ``fail_to_backlog`` (default): the queued dedicated runs fail with
    ``dedicated_runner_revoked``; the assignment is cleared; a ticking
    issue moves to Backlog through ``_bounce_issue_no_eligible_runner``
    (reason ``dedicated-runner-revoked``). An issue in another state only
    gets the cleared assignment and comments.

    ``fall_back``: routed to a squad — its own pod when that pod has a
    runner that could serve the issue, else the project's default pod
    (``assigned_pod`` set in the same transaction; the queued run is
    cancelled and on commit recreated in the default pod), else the
    existing no-eligible-runner bounce. The replacement's check and the
    bounce share ``pod_has_runner_for_issue_principal``'s pod branch, so
    steps 2 and 3 cannot disagree.
    """
    from pi_dash.orchestration.agent_phases import is_ticking_state
    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    issue.refresh_from_db(fields=["assigned_runner"])
    runner = issue.assigned_runner
    runner_name = runner.name if runner is not None else "the dedicated runner"
    queued = list(
        AgentRun.objects.select_for_update().filter(
            work_item=issue, pin_kind=PinKind.DEDICATED, status=AgentRunStatus.QUEUED
        )
    )
    policy = getattr(issue.project, "dedicated_runner_lost_action", "fail_to_backlog")
    if policy == "fall_back":
        _apply_fall_back(issue, queued, runner_name=runner_name)
        return
    for run in queued:
        finalize_agent_run(
            run.pk,
            AgentRunStatus.FAILED,
            expected_status=AgentRunStatus.QUEUED,
            updates={
                "error_code": "dedicated_runner_revoked",
                "error": _lost_runner_revoke_error(runner_name),
            },
        )
    clear_dedicated_runner(
        issue, reason="runner lost — run failed, issue moved to backlog", actor=None, decided_by=None
    )
    if is_ticking_state(issue.state if issue.state_id else None):
        from pi_dash.orchestration.scheduling import _bounce_issue_no_eligible_runner

        issue.refresh_from_db(fields=["state", "assigned_runner"])
        _bounce_issue_no_eligible_runner(
            issue,
            triggered_by=reason,
            reason="dedicated-runner-revoked",
            reason_name=runner_name,
        )


def _apply_fall_back(issue, queued, *, runner_name):
    """``fall_back`` arm of :func:`apply_lost_runner_policy` (§13.1).

    At most one non-terminal run exists per issue, so the queued run (if
    any) stands in for the could-serve check; with no queued run the
    check runs with ``run_creator_id=None, human=False`` as §13.1 says.
    """
    from django.db import transaction

    from pi_dash.runner.models import AgentRunStatus, PinKind
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run
    from pi_dash.runner.services.matcher import pod_has_runner_for_issue_principal

    from pi_dash.runner.models import HUMAN_TRIGGERS, Pod

    run = queued[0] if queued else None

    def _could_serve(pod):
        # "A runner that could serve the issue": the pod branch.
        # pod_once=True forces that branch even though the assignment is
        # still set at this point.
        if pod is None:
            return False
        if run is not None:
            creator = run.created_by_id
            human = run.trigger in HUMAN_TRIGGERS if run.trigger else True
        else:
            creator = None
            human = False
        return pod_has_runner_for_issue_principal(pod, issue, creator, human=human, pod_once=True)

    own_pod = issue.assigned_pod
    if own_pod is not None and _could_serve(own_pod):
        # Step 1: own pod has a runner that could serve the issue.
        for r in queued:
            r.pinned_runner = None
            r.pin_kind = PinKind.LOCALITY
            r.save(update_fields=["pinned_runner", "pin_kind"])
        clear_dedicated_runner(issue, reason="runner lost — fell back to the pod", actor=None, decided_by=None)
        _post_fall_back_comment(issue, runner_name=runner_name, pod=own_pod)
        return

    default_pod = Pod.default_for_project(issue.project)
    other_pod = default_pod is not None and str(default_pod.pk) != str(own_pod.pk if own_pod else "")
    if other_pod and _could_serve(default_pod):
        # Step 2: the project's default pod has one. assigned_pod moves in
        # the same transaction; the queued run (AgentRun.pod is immutable)
        # is cancelled and on commit recreated in the default pod.
        issue.assigned_pod = default_pod
        issue.save(update_fields=["assigned_pod", "updated_at"])
        for r in queued:
            if finalize_agent_run(
                r.pk,
                AgentRunStatus.CANCELLED,
                expected_status=AgentRunStatus.QUEUED,
                updates={"error": "runner revoked; replaced in the default pod", "error_code": "runner_revoked"},
            ):
                issue_id, parent_id = issue.id, r.id
                trigger = r.trigger

                def _recreate(issue_id=issue_id, parent_id=parent_id, trigger=trigger, pod_id=default_pod.pk):
                    from pi_dash.db.models.issue import Issue as _Issue
                    from pi_dash.orchestration.service import _create_and_dispatch_run
                    from pi_dash.runner.models import AgentRun as _Run
                    from pi_dash.runner.models import Pod as _Pod

                    _issue = _Issue.objects.filter(pk=issue_id).first()
                    _parent = _Run.objects.filter(pk=parent_id).first()
                    _pod = _Pod.objects.filter(pk=pod_id).first()
                    if _issue is None or _parent is None or _pod is None:
                        return
                    _creator = _parent.created_by
                    if _creator is None:
                        return
                    _create_and_dispatch_run(
                        issue=_issue,
                        parent=_parent,
                        creator=_creator,
                        pod=_pod,
                        fresh_session=False,
                        trigger=trigger,
                        pod_once=False,
                    )

                transaction.on_commit(_recreate)
        clear_dedicated_runner(issue, reason="runner lost — fell back to the default pod", actor=None, decided_by=None)
        _post_fall_back_comment(issue, runner_name=runner_name, pod=default_pod)
        return

    # Step 3: no squad can serve the issue — the existing bounce applies.
    # The runner is gone for good, so waiting cannot help (§13.1): the
    # queued run fails with dedicated_runner_revoked, as in fail_to_backlog.
    for r in queued:
        finalize_agent_run(
            r.pk,
            AgentRunStatus.FAILED,
            expected_status=AgentRunStatus.QUEUED,
            updates={
                "error_code": "dedicated_runner_revoked",
                "error": _lost_runner_revoke_error(runner_name),
            },
        )
    clear_dedicated_runner(issue, reason="runner lost — no runner left in the pod", actor=None, decided_by=None)
    from pi_dash.orchestration.agent_phases import is_ticking_state
    from pi_dash.orchestration.scheduling import _bounce_issue_no_eligible_runner

    if is_ticking_state(issue.state if issue.state_id else None):
        issue.refresh_from_db(fields=["state"])
        _bounce_issue_no_eligible_runner(
            issue,
            triggered_by="runner_revoked",
            reason="no-eligible-runner",
        )


def _post_fall_back_comment(issue, *, runner_name, pod):
    """System comment for ``fall_back`` naming runner, reason and pod (§13.1)."""
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user

    IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=get_agent_system_user(),
        comment_html=format_html(
            "<p>Runner <strong>{}</strong> is gone. The issue now uses pod <strong>{}</strong>.</p>",
            runner_name,
            pod.name if pod is not None else "the pod",
        ),
        speaker_type=IssueComment.SpeakerType.SYSTEM,
    )

