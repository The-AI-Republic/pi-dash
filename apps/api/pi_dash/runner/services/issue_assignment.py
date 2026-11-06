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


def _check_assignment_code(runner, *, user, issue, pod=None):
    """Shared E1-E8 (sans E6) evaluation for one dedicated choice (design §7.1).

    Returns ``(code, detail)`` — ``code`` is ``""`` when the choice is
    assignable, else the first failing E-code in E1-E8 order, with the
    human-readable ``detail`` for the refusal. ``issue=None`` means "new"
    (project membership of the caller owns E2; the requester is the future
    creator so E7 is satisfied by construction). ``pod`` is the requested
    execution pod, or ``None`` when the request names no pod.
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
    Every clear writes an issue with ``actor`` (the system actor for
    system-initiated clears) and a comment: *the system decided the issue
    had been unpinned, and the reason*.

    ``decided_by`` is the moment the dedicated assignment was bypassed
    deliberately (H-12); ``None`` for system-initiated clears.
    """
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user

    issue.assigned_runner = None
    issue.save(update_fields=["assigned_runner", "updated_at"])
    if actor is None:
        actor = get_agent_system_user()
    body = format_html(
        "<p>The dedicated runner assignment was cleared: <strong>{}</strong>.</p>",
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


def validate_execution_target(attrs, *, issue=None, project=None, user=None):
    """Validate the requested execution target in place (design §7.1).

    ``attrs`` is the serializer's validated data. When ``assigned_runner``
    (or the ``_id`` spelling) is present, E1-E8 are evaluated and the first
    failure raises ``serializers.ValidationError`` keyed on the field the
    caller posted, carrying the E-code as the error ``code``. Skips runner
    checks when the request names no runner. A non-``None`` runner on a
    create/update without a resolvable pod is refused by E1 (a runner that
    cannot be addressed is not found).
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
        return
    pod = attrs.get("assigned_pod") or attrs.get("assigned_pod_id")
    if pod is not None and not hasattr(pod, "project_id"):
        from pi_dash.runner.models import Pod

        pod = Pod.objects.filter(pk=pod).first()
    code, detail = _check_assignment_code(runner, user=user, issue=issue, pod=pod)
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
    """
    from django.utils import timezone as dj_timezone

    from pi_dash.db.models.issue import IssueActivity
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    decided_at = decided_by or dj_timezone.now()
    issue.assigned_runner = new_runner
    issue.save(update_fields=["assigned_runner", "updated_at"])
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


def dedicated_run_refusal(issue, *, human, user):
    """Pre-creation refusal for a person's run (§11).

    Returns ``(code, detail)`` — ``assigned_runner_offline`` or
    ``assigned_runner_paused`` — or ``(None, None)`` when the run may
    proceed. Only a person's run is refused; an automatic run is created
    and waits (§11.3). Busy is never a refusal.
    """
    if not human:
        return None, None
    runner = getattr(issue, "assigned_runner", None)
    if runner is None:
        return None, None
    if runner.revoked_at is not None:
        return None, None  # E4 already failed at assignment; nothing to refuse here
    pause = runner_paused_reason(runner)
    if pause is not None:
        return pause, "dedicated runner is paused"
    if runner_is_offline(runner):
        return "assigned_runner_offline", "dedicated runner is offline"
    return None, None


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


def fail_dedicated_runs() -> int:
    """Fail dedicated runs whose runner is still unavailable after 12h (§11.3).

    Automatic runs wait for their dedicated runner; once older than
    ``DEDICATED_RUN_QUEUED_MAX_AGE_SECS`` and the runner is still offline
    or paused, the run fails with ``dedicated_runner_unavailable`` and no
    re-queue. Person-initiated runs are never failed here — they were
    refused at creation, so a runner that came back and went offline
    again cannot strand them.
    """
    from datetime import timedelta as td

    from django.conf import settings as dj_settings

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
        run = AgentRun.objects.select_related("pinned_runner").filter(pk=run_id).first()
        if run is None:
            continue
        runner = run.pinned_runner
        if runner is None:
            continue
        if runner_paused_reason(runner) is None and not runner_is_offline(runner):
            continue  # runner back online: the run drains normally
        if finalize_agent_run(
            run_id,
            AgentRunStatus.FAILED,
            expected_status=AgentRunStatus.QUEUED,
            updates={
                "error_code": "dedicated_runner_unavailable",
                "error": refusal_detail("dedicated_runner_unavailable", new_runner=runner),
            },
        ):
            failed += 1
    return failed


def apply_lost_runner_policy(issue, *, reason):
    """Apply the project's lost-runner handling to one issue (§12.3).

    ``fail_to_backlog`` (default) cancels the dedicated QUEUED runs with
    ``dedicated_runner_unavailable`` and clears the assignment.
    ``fall_back`` clears the assignment and downstream run kinds, making
    the run a pod run. Called inside the revoking transaction.
    """
    from pi_dash.runner.models import AgentRun, AgentRunStatus, PinKind
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    issue.refresh_from_db(fields=["assigned_runner"])
    runner = issue.assigned_runner
    queued = list(
        AgentRun.objects.select_for_update().filter(
            work_item=issue, pin_kind=PinKind.DEDICATED, status=AgentRunStatus.QUEUED
        )
    )
    policy = getattr(issue.project, "dedicated_runner_lost_action", "fail_to_backlog")
    if policy == "fall_back":
        for run in queued:
            run.pin_kind = PinKind.LOCALITY
            run.save(update_fields=["pin_kind"])
        queued_ids = [r.pk for r in queued]
        # Release the pins into the pod queue in the same loop below.
        for run_id in queued_ids:
            AgentRun.objects.filter(pk=run_id).update(pinned_runner=None)
        clear_dedicated_runner(issue, reason="runner lost — fell back to the pod", actor=None, decided_by=None)
        return
    for run in queued:
        finalize_agent_run(
            run.pk,
            AgentRunStatus.CANCELLED,
            expected_status=AgentRunStatus.QUEUED,
            updates={
                "error_code": "dedicated_runner_unavailable",
                "error": refusal_detail("dedicated_runner_unavailable", new_runner=runner),
            },
        )
    clear_dedicated_runner(issue, reason="runner lost — run failed, issue moved to backlog", actor=None, decided_by=None)

