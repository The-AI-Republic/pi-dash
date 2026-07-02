# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic agent ticking — primitives for the per-issue ticker clock.

This module owns the per-issue ``IssueAgentTicker`` row: when to arm it,
when to disarm it, when to reset it, how to dispatch a continuation run on
a tick, and how to apply the deferred cap-hit pause.

This is internal continuation-cadence machinery, system-armed on Issue
state transitions; it is not a user-authored periodic-job system.

See ``.ai_design/issue_ticking_system/design.md`` for the full design;
quick links:

- §4 lifecycle (arm / disarm / re-arm)
- §4.4.1 deferred cap-hit pause
- §4.6 Comment & Run reset
- §6 scanner + atomic claim
- §12.2 public API surface
"""

from __future__ import annotations

import logging
from datetime import timedelta
from typing import Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.issue_agent_ticker import (
    DEFAULT_INTERVAL_SECONDS,
    DEFAULT_MAX_TICKS,
    INFINITE_MAX_TICKS,
    IssueAgentTicker,
    TickerDisarmReason,
    jitter_seconds,
)
from pi_dash.orchestration.agent_phases import is_ticking_state
from pi_dash.runner.models import AgentRun, AgentRunTrigger

logger = logging.getLogger(__name__)


PAUSED_STATE_NAME = "Paused"

# DEPRECATED: retained only for backward compatibility with external
# importers (tests, integrations). Internal callers must use
# ``orchestration.agent_phases.is_ticking_state`` /
# ``phase_config_for`` — the literal still names the In Progress
# phase's state, see ``agent_phases.PHASES``. Remove once no
# remaining imports of ``DELEGATION_STATE_NAME`` exist.
DELEGATION_STATE_NAME = "In Progress"


# ---------------------------------------------------------------------------
# Arming / disarming
# ---------------------------------------------------------------------------


def _project_default_interval(issue: Issue) -> int:
    """Project-level default interval for the issue's *current* phase.

    Used for the brand-new ticker row's first ``next_run_at`` compute,
    where there is no row yet to ask ``effective_interval_seconds``.
    Subsequent arms read directly from the row's phase-aware
    ``effective_*`` methods.
    """
    from pi_dash.db.models.state import StateGroup

    project = issue.project
    state = getattr(issue, "state", None)
    if state is not None and state.group == StateGroup.REVIEW.value:
        return getattr(
            project,
            "agent_review_default_interval_seconds",
            DEFAULT_INTERVAL_SECONDS,
        )
    return getattr(project, "agent_default_interval_seconds", DEFAULT_INTERVAL_SECONDS)


def _project_default_max_ticks(issue: Issue) -> int:
    from pi_dash.db.models.state import StateGroup

    project = issue.project
    state = getattr(issue, "state", None)
    if state is not None and state.group == StateGroup.REVIEW.value:
        return getattr(
            project,
            "agent_review_default_max_ticks",
            DEFAULT_MAX_TICKS,
        )
    return getattr(project, "agent_default_max_ticks", DEFAULT_MAX_TICKS)


def _project_ticking_enabled(issue: Issue) -> bool:
    project = issue.project
    return bool(getattr(project, "agent_ticking_enabled", True))


def _compute_next_run_at(interval_seconds: int, *, base=None):
    base = base or timezone.now()
    return base + timedelta(seconds=interval_seconds + jitter_seconds(interval_seconds))


def arm_ticker(
    issue: Issue,
    *,
    dispatch_immediate: bool = True,  # noqa: ARG001 — caller-only signal
) -> IssueAgentTicker:
    """Create or reset the ticker for an issue entering a ticking state.

    ``dispatch_immediate`` does not affect the schedule itself — arming
    *never* fires a run. The flag exists only to document the caller's
    intent: ``True`` means the caller is firing (or expects another
    immediate dispatch), ``False`` means arming alone is enough — no
    new run should be inferred from this call. Used by Comment & Run
    on a Paused issue (§4.6) and by re-arm-on-comment in
    :func:`pi_dash.orchestration.service.handle_issue_comment`.

    Honors ``user_disabled`` and project-level ``agent_ticking_enabled``:
    sets ``enabled = False`` when either suppresses ticks.
    """
    suppress_for_project = not _project_ticking_enabled(issue)

    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            # Brand-new row. We need a sensible ``next_run_at`` before
            # we can ask for ``effective_interval_seconds`` (which
            # consults the row), so use the project default for the
            # issue's *current* phase. ``_project_default_interval``
            # picks the review-phase default when the issue's current
            # state group is REVIEW (e.g., a Todo → In Review path that
            # skips the impl phase) and the In Progress default
            # otherwise. Subsequent arms use the row's phase-aware
            # ``effective_interval_seconds`` directly.
            interval = _project_default_interval(issue)
            sched = IssueAgentTicker.objects.create(
                issue=issue,
                interval_seconds=None,
                max_ticks=None,
                review_interval_seconds=None,
                review_max_ticks=None,
                user_disabled=False,
                next_run_at=_compute_next_run_at(interval),
                tick_count=0,
                enabled=not suppress_for_project,
                disarm_reason=TickerDisarmReason.NONE,
            )
        else:
            # Existing row — reset clock and re-evaluate enabled.
            sched.tick_count = 0
            sched.next_run_at = _compute_next_run_at(
                sched.effective_interval_seconds()
            )
            suppress = sched.user_disabled or suppress_for_project
            sched.enabled = not suppress
            # Re-arming clears any prior disarm cause; if we end up
            # disabled because of user_disabled or project-suppress,
            # the next disarm caller will set the appropriate reason.
            sched.disarm_reason = TickerDisarmReason.NONE
            sched.save(
                update_fields=[
                    "tick_count",
                    "next_run_at",
                    "enabled",
                    "disarm_reason",
                    "updated_at",
                ]
            )

    logger.info(
        "agent_ticker: armed issue=%s enabled=%s next_run_at=%s",
        issue.pk,
        sched.enabled,
        sched.next_run_at,
    )
    return sched


def disarm_ticker(
    issue: Issue,
    *,
    reason: str = TickerDisarmReason.LEFT_TICKING_STATE,
) -> Optional[IssueAgentTicker]:
    """Set ``enabled = False`` and persist ``disarm_reason``. Idempotent.

    Called when issue leaves a ticking group, on cap hit, on terminal
    done-signal, and when the user toggles ``user_disabled = True``
    mid-flight. Returns the schedule if one exists; ``None`` otherwise.

    The ``reason`` argument is **load-bearing**: only ``CAP_HIT``
    triggers ``maybe_apply_deferred_pause`` to auto-Pause the issue.
    Terminal-signal disarms (``completed`` / ``blocked``) leave the
    issue in place for the human to act.

    Note: this helper *overwrites* ``disarm_reason`` even when the
    ticker is already disabled — callers are expected to know which
    cause should win. This differs from
    :func:`maybe_disarm_on_terminal_signal`, which deliberately
    preserves the prior reason so an opportunistic terminal-signal
    disarm cannot clobber a pre-existing ``CAP_HIT``. Today the only
    production caller is the state-transition handler with the
    default ``LEFT_TICKING_STATE`` reason. Callers needing the
    terminal-signal semantics must use
    :func:`maybe_disarm_on_terminal_signal`; passing
    ``TERMINAL_SIGNAL`` here raises ``ValueError`` because the
    overwrite-always behavior would silently clobber a pre-existing
    ``CAP_HIT`` and skip the auto-pause.
    """
    if reason == TickerDisarmReason.TERMINAL_SIGNAL:
        raise ValueError(
            "disarm_ticker overwrites disarm_reason — use "
            "maybe_disarm_on_terminal_signal() for TERMINAL_SIGNAL "
            "to preserve a prior CAP_HIT and the auto-pause it gates."
        )
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return None
        # Always update the reason — even when already disabled — so a
        # later disarm with a different reason (e.g., terminal signal
        # arriving on an already cap-hit-disabled ticker) does not
        # incorrectly preserve the older reason. But the rule is that
        # the *first* terminal cause wins for the auto-pause gate, so
        # only the first transition flips ``enabled``.
        changed = False
        if sched.enabled:
            sched.enabled = False
            changed = True
        if sched.disarm_reason != reason:
            sched.disarm_reason = reason
            changed = True
        if changed:
            sched.save(
                update_fields=["enabled", "disarm_reason", "updated_at"]
            )
    logger.info(
        "agent_ticker: disarmed issue=%s reason=%s", issue.pk, reason
    )
    return sched


def maybe_disarm_on_terminal_signal(run: AgentRun) -> bool:
    """Disarm the ticker if the run's done-payload status is a phase-
    final signal (``completed`` or ``blocked``).

    Do not disarm on ``noop`` (which currently persists as
    ``AgentRunStatus.COMPLETED``). The hook inspects
    ``run.done_payload['status']`` rather than ``run.status`` because
    ``noop`` and true completion share the same persisted run status.

    **Critical:** only disarm when the ticker is currently *armed*. If
    the ticker is already disabled, leave the existing
    ``disarm_reason`` alone — that prior reason (typically ``CAP_HIT``
    when cap was hit during the same tick that produced this run)
    determines whether ``maybe_apply_deferred_pause`` will auto-Pause.
    Overwriting a ``CAP_HIT`` reason with ``TERMINAL_SIGNAL`` would
    silently drop the auto-pause.

    Closes the gap in the existing issue-ticking design: the spec
    says terminal completed/blocked should disarm but the existing
    code only disarms via state transitions. Idempotent. Safe to
    call alongside ``maybe_apply_deferred_pause``.

    Returns ``True`` when a disarm was applied this call (i.e., the
    ticker was armed and is now disabled with reason
    ``TERMINAL_SIGNAL``).
    """
    if run.work_item_id is None:
        return False
    payload = run.done_payload or {}
    payload_status = (payload or {}).get("status")
    if payload_status not in {"completed", "blocked"}:
        return False
    issue = run.work_item
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return False
        # Already disabled — preserve the prior reason. See docstring.
        if not sched.enabled:
            return False
        sched.enabled = False
        sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
        sched.save(
            update_fields=["enabled", "disarm_reason", "updated_at"]
        )
    logger.info(
        "agent_ticker: disarmed-on-terminal issue=%s payload_status=%s",
        issue.pk,
        payload_status,
    )
    return True


def reset_ticker_after_comment_and_run(issue: Issue) -> Optional[IssueAgentTicker]:
    """Reset ``tick_count = 0`` and ``next_run_at = NOW() + interval + jitter``.

    Called by the Comment & Run handler after the run is dispatched
    (§4.6 step 4). Uses ``select_for_update`` to serialize against
    ``fire_tick`` (§6.1).
    """
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return None
        sched.tick_count = 0
        sched.next_run_at = _compute_next_run_at(sched.effective_interval_seconds())
        # Comment & Run is an explicit re-engagement — re-enable unless
        # the user has explicitly disabled ticking on this issue.
        suppress = sched.user_disabled or not _project_ticking_enabled(issue)
        sched.enabled = not suppress
        sched.disarm_reason = TickerDisarmReason.NONE
        sched.save(
            update_fields=[
                "tick_count",
                "next_run_at",
                "enabled",
                "disarm_reason",
                "updated_at",
            ]
        )
    logger.info(
        "agent_ticker: reset issue=%s next_run_at=%s",
        issue.pk,
        sched.next_run_at,
    )
    return sched


def re_tick_ticker(issue: Issue) -> dict:
    """Grant a fresh phase-sized tick budget to an exhausted ticker and re-arm.

    Manual "re-ticking". When a ticker has burned through its budget
    (``cap_reached()``) while the issue is still in a ticking state
    (In Progress / In Review), the user can re-grant budget so the
    continuation clock resumes. Unlike
    :func:`reset_ticker_after_comment_and_run` (which zeroes
    ``tick_count``), re-ticking **grows** the cap by one fresh phase
    budget and leaves ``tick_count`` intact — the issue detail card keeps
    showing cumulative progress ("Tick 24 of 48") rather than resetting to
    "Tick 0 of 24".

    The grant amount is the project default for the issue's *current*
    phase (In Progress vs In Review), so re-ticking In Review re-grants the
    In-Review budget and re-ticking In Progress re-grants the In-Progress
    budget. Because ``tick_count == effective_max_ticks()`` at exhaustion,
    ``new_cap = effective_max_ticks() + grant`` yields exactly ``grant``
    fresh ticks. Using the phase's *project default* as the grant unit
    keeps repeated re-ticks granting a stable amount even though the grant
    is persisted by bumping the phase cap override.

    Returns ``{"granted": bool, "reason": str, "ticker": IssueAgentTicker|None}``.
    All three guardrails below must hold or the call is a no-op with
    ``granted = False`` and no mutation:

    * a ticker row exists (``reason = "no_ticker"``),
    * the issue is currently in a ticking state (``reason =
      "not_ticking_state"``),
    * the budget is actually exhausted (``reason =
      "budget_not_exhausted"``).

    ``select_for_update`` serializes against ``fire_tick`` and the deferred
    cap-hit pause (§6.1 / §4.4.1).
    """
    with transaction.atomic():
        # Re-fetch and lock the issue inside the transaction so the state
        # guard below sees the latest committed state, not a possibly-stale
        # instance handed in by the caller. Without this, a concurrent
        # transition out of a ticking state (e.g. to Done) could race the
        # re-arm and leave the ticker enabled for a non-ticking issue.
        locked_issue = (
            Issue.all_objects.select_for_update(of=("self",))
            .select_related("state", "project")
            .filter(pk=issue.pk)
            .first()
        )
        if locked_issue is None:
            return {"granted": False, "reason": "no_issue", "ticker": None}

        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=locked_issue)
            .first()
        )
        if sched is None:
            return {"granted": False, "reason": "no_ticker", "ticker": None}
        # Bind the freshly-locked issue so phase-aware methods
        # (``_is_review_phase``/``effective_max_ticks``) resolve against the
        # current state rather than lazy-loading a fresh copy.
        sched.issue = locked_issue
        if not is_ticking_state(locked_issue.state):
            return {"granted": False, "reason": "not_ticking_state", "ticker": sched}
        if not sched.cap_reached():
            return {"granted": False, "reason": "budget_not_exhausted", "ticker": sched}

        grant = _project_default_max_ticks(locked_issue)
        review = sched._is_review_phase()
        new_cap = sched.effective_max_ticks() + grant
        if review:
            sched.review_max_ticks = new_cap
        else:
            sched.max_ticks = new_cap

        # Re-arm: clear the disarm cause and restart the clock unless the
        # user disabled ticking on this issue or the project suppresses it.
        suppress = sched.user_disabled or not _project_ticking_enabled(locked_issue)
        sched.enabled = not suppress
        sched.disarm_reason = TickerDisarmReason.NONE
        sched.next_run_at = _compute_next_run_at(sched.effective_interval_seconds())
        sched.save(
            update_fields=[
                "review_max_ticks" if review else "max_ticks",
                "enabled",
                "disarm_reason",
                "next_run_at",
                "updated_at",
            ]
        )
    logger.info(
        "agent_ticker: re-ticked issue=%s new_cap=%s enabled=%s next_run_at=%s",
        locked_issue.pk,
        new_cap,
        sched.enabled,
        sched.next_run_at,
    )
    return {"granted": True, "reason": "granted", "ticker": sched}


# ---------------------------------------------------------------------------
# Continuation dispatch
# ---------------------------------------------------------------------------


# Sourced from ``AgentRunTrigger`` so a value change propagates here rather
# than silently diverging — these strings are passed straight to
# ``AgentRun.trigger`` and feed ``run_is_human_triggered`` (design §9.1).
TRIGGER_TICK = AgentRunTrigger.TICK.value
TRIGGER_COMMENT_AND_RUN = AgentRunTrigger.COMMENT_AND_RUN.value
TRIGGER_RUN_AI = AgentRunTrigger.RUN_AI.value


def _resolve_pod_for_issue(issue: Issue):
    from pi_dash.runner.models import Pod

    if issue.assigned_pod_id is not None:
        pinned = Pod.objects.filter(pk=issue.assigned_pod_id).first()
        if pinned is not None:
            return pinned
    # Pods are project-scoped — fall back to the issue's project default.
    # Issues without a project (shouldn't exist post-refactor) return None
    # and the caller surfaces an error rather than silently routing into
    # a workspace-wide pod.
    if issue.project_id is None:
        return None
    return Pod.default_for_project_id(issue.project_id)


def _resolve_creator_for_trigger(issue: Issue, *, triggered_by: str, actor=None):
    """Pick the user who will appear as ``AgentRun.created_by``."""
    if actor is not None:
        return actor
    if triggered_by == TRIGGER_TICK:
        # System-driven; attribute to the agent system bot.
        from pi_dash.orchestration.workpad import get_agent_system_user
        return get_agent_system_user()
    # Fall back to the issue creator / project lead — same logic the
    # state-transition path uses.
    if issue.created_by_id:
        return issue.created_by
    project = issue.project
    return project.project_lead or project.default_assignee


def preflight_eligibility_or_bounce(
    issue: Issue, *, run_creator, pod, triggered_by: str
) -> bool:
    """Return True if dispatch can proceed; False if the issue was bounced.

    Companion preflight to the four issue-run dispatch paths. When no
    runner registered in ``pod`` has an owner the matcher's
    ``filter_runs_usable_by_runner`` would accept for a run on ``issue``
    created by ``run_creator``, this moves the issue back to its
    project's Backlog state and posts a system comment explaining why.
    The caller must NOT create the ``AgentRun`` when this returns False.

    See ``.ai_design/issue_runner/design.md`` §6.6.
    """
    from pi_dash.runner.services.matcher import (
        pod_has_runner_for_issue_principal,
    )

    creator_id = getattr(run_creator, "id", None)
    if pod_has_runner_for_issue_principal(pod, issue, creator_id):
        return True
    _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by)
    return False


def _bounce_issue_no_eligible_runner(issue: Issue, *, triggered_by: str) -> None:
    """Move ``issue`` back to Backlog and post the no-eligible-runner notice.

    State move fires ``fire_state_transition`` which disarms the ticker as
    a side-effect (Backlog isn't a delegation trigger). Skipped when the
    issue is already in the BACKLOG state group — the comment still posts
    so the user sees *why* a click / tick produced no run.

    Target resolution (design §6.6 step 1): prefer the project's Backlog
    state (``default`` first, then ``sequence``); if the project has no
    Backlog state at all, fall back to ``project.default_state`` **only
    when it is not itself a ticking state** — moving into a ticking state
    would re-fire dispatch and bounce again in a loop. When neither a
    Backlog nor a safe fallback exists, the issue is left in place and the
    ticker is disarmed explicitly (below) so the next tick can't re-enter
    this bounce forever.

    The state move + comment post are wrapped in a single atomic block
    so a partial bounce (state changed, no comment) can't survive a
    crash mid-write — the user would otherwise be staring at an issue
    that silently jumped back to Backlog with no explanation.
    """
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.db.models.state import State, StateGroup
    from pi_dash.orchestration.workpad import get_agent_system_user

    logger.info(
        "agent_dispatch: bounce issue=%s reason=no-eligible-runner triggered_by=%s",
        issue.pk,
        triggered_by,
    )

    body = format_html(
        "<p><strong>Agent run skipped — no eligible runner.</strong></p>"
        "<p>No runner is registered in this pod that can serve this issue. "
        "Add a runner under your account, or assign this issue to a "
        "workspace member whose runner is registered here.</p>"
    )

    with transaction.atomic():
        current_state_group = issue.state.group if issue.state_id else None
        if current_state_group != StateGroup.BACKLOG.value:
            target_state = (
                State.objects.filter(
                    project_id=issue.project_id,
                    group=StateGroup.BACKLOG.value,
                )
                .order_by("-default", "sequence")
                .first()
            )
            if target_state is None:
                # Defensive: DEFAULT_STATES seeds a Backlog state for every
                # project, but if one is missing fall back to the project's
                # default_state — only when it isn't itself a ticking state,
                # since moving into a ticking state re-fires dispatch and
                # re-bounces (design §6.6 step 1).
                fallback = issue.project.default_state
                if fallback is not None and not is_ticking_state(fallback):
                    target_state = fallback
                else:
                    logger.warning(
                        "agent_dispatch: no safe backlog target for "
                        "project=%s; issue=%s stays in current state, "
                        "ticker disarmed",
                        issue.project_id,
                        issue.pk,
                    )
            if target_state is not None and target_state.pk != issue.state_id:
                issue.state = target_state
                issue.save(update_fields=["state", "updated_at"])

        # If the issue couldn't be moved out of a ticking state, the
        # state-move signal never disarmed the ticker — do it explicitly so
        # a subsequent tick doesn't re-enter this bounce endlessly, spamming
        # a comment each time.
        if is_ticking_state(issue.state if issue.state_id else None):
            disarm_ticker(issue)

        IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=get_agent_system_user(),
            comment_html=body,
            speaker_type=IssueComment.SpeakerType.AGENT,
        )


def dispatch_continuation_run(
    issue: Issue,
    *,
    triggered_by: str,
    actor=None,
) -> Optional[AgentRun]:
    """Public wrapper for tick / Comment & Run dispatch.

    Resolves parent (latest prior run), creator (system bot for ticks;
    explicit ``actor`` for Comment & Run), pod, then delegates to
    :func:`pi_dash.orchestration.service._create_continuation_run`.
    Returns the created run, or ``None`` when the single-active-run
    guardrail blocks creation, no pod is available, or the eligibility
    preflight bounced the issue (§6.6).
    """
    from pi_dash.orchestration import service as orchestration_service

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    parent = orchestration_service._latest_prior_run(issue)
    if parent is None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=no-prior-run triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    creator = _resolve_creator_for_trigger(
        issue, triggered_by=triggered_by, actor=actor
    )
    if creator is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    if not preflight_eligibility_or_bounce(
        issue, run_creator=creator, pod=pod, triggered_by=triggered_by
    ):
        return None

    outcome = orchestration_service._create_continuation_run(
        issue=issue,
        parent=parent,
        creator=creator,
        pod=pod,
        trigger=triggered_by,
    )
    return outcome.created_run


def dispatch_run_ai_run(issue: Issue, *, actor) -> Optional[AgentRun]:
    """Public wrapper for the "Run AI" button.

    Builds the same templated prompt the state-transition-into-In-Progress
    path produces, by routing through the orchestration service's run-
    creation helpers (which call ``composer.build_first_turn``). This is
    the prompt-parity contract: a manual Run AI click renders the phase's
    template against the issue's current state, identical to a tick or a
    state transition into the same phase.

    Behavior:
    - Bails (returns ``None``) when an active run already exists on the
      issue (single-active-run guardrail) or no pod is available.
    - When a prior run exists, delegates to ``_create_continuation_run``
      so the new run inherits parent linkage and runner pinning (repo
      locality, same as Comment & Run / tick).
    - When no prior run exists, delegates to ``_create_and_dispatch_run``
      so a brand-new issue's first agent run still goes through the
      templated prompt path.
    """
    from pi_dash.orchestration import service as orchestration_service

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None

    creator = _resolve_creator_for_trigger(
        issue, triggered_by=TRIGGER_RUN_AI, actor=actor
    )
    if creator is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None

    if not preflight_eligibility_or_bounce(
        issue, run_creator=creator, pod=pod, triggered_by=TRIGGER_RUN_AI
    ):
        return None

    parent = orchestration_service._latest_prior_run(issue)
    if parent is not None:
        outcome = orchestration_service._create_continuation_run(
            issue=issue,
            parent=parent,
            creator=creator,
            pod=pod,
            trigger=TRIGGER_RUN_AI,
        )
    else:
        outcome = orchestration_service._create_and_dispatch_run(
            issue=issue,
            parent=None,
            creator=creator,
            pod=pod,
            trigger=TRIGGER_RUN_AI,
        )
    return outcome.created_run


# ---------------------------------------------------------------------------
# Deferred cap-hit pause (§4.4.1)
# ---------------------------------------------------------------------------


def maybe_apply_deferred_pause(run: AgentRun) -> bool:
    """If the schedule was disarmed by **cap exhaustion**, the issue is
    still in a ticking state, and no other active runs exist on the
    issue, transition the issue → Paused.

    Idempotent — only the first concurrent terminate event takes effect.
    Returns ``True`` when a transition was applied, ``False`` otherwise.

    Gated on ``disarm_reason == CAP_HIT``: terminal-signal disarms
    (``completed``/``blocked``) leave the issue in place for the human
    to act. ``LEFT_TICKING_STATE`` and ``USER_DISABLED`` likewise are
    not auto-pause causes.

    Called from the runner Channels consumer after persisting a terminal
    run status.
    """
    if run.work_item_id is None:
        return False

    issue = run.work_item
    sched = IssueAgentTicker.objects.filter(issue=issue).first()
    if sched is None or sched.enabled:
        return False
    if sched.disarm_reason != TickerDisarmReason.CAP_HIT:
        return False

    state = issue.state
    if state is None:
        return False
    if not is_ticking_state(state):
        return False

    # Other active runs (besides the one that just terminated) keep the
    # issue alive in In Progress — the next terminate will check again.
    from pi_dash.orchestration import service as orchestration_service

    active = orchestration_service._active_run_for(issue)
    if active is not None and active.pk != run.pk:
        return False

    paused_state = (
        type(state).all_state_objects.filter(
            project=issue.project,
            name=PAUSED_STATE_NAME,
            deleted_at__isnull=True,
        )
        .first()
    )
    if paused_state is None:
        logger.warning(
            "agent_ticker: cannot auto-pause issue=%s — no Paused state in project",
            issue.pk,
        )
        return False

    from pi_dash.orchestration.workpad import get_agent_system_user

    bot = get_agent_system_user()
    with transaction.atomic():
        # Re-fetch the schedule under a row lock so the disarmed-check we
        # made above stays valid for the rest of this transaction. Without
        # this, a concurrent ``arm_ticker`` (e.g. user manually re-starts
        # the issue between the unlocked read on line ~314 and here) can
        # re-enable the schedule while we auto-pause its issue.
        locked_sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(pk=sched.pk)
            .first()
        )
        if locked_sched is None or locked_sched.enabled:
            return False
        # Re-check disarm_reason under the lock so a concurrent
        # ``disarm_ticker`` (e.g., the user moved the issue out of the
        # ticking state mid-flight, flipping the reason from CAP_HIT to
        # LEFT_TICKING_STATE) cannot drive an auto-pause off a stale
        # unlocked read.
        if locked_sched.disarm_reason != TickerDisarmReason.CAP_HIT:
            return False
        # Re-fetch the issue under the same transaction to guard against a
        # racing state transition.
        IssueModel = type(issue)
        locked = (
            IssueModel.all_objects.select_for_update().filter(pk=issue.pk).first()
        )
        if locked is None:
            return False
        if locked.state_id != state.id:
            return False
        locked.state = paused_state
        if hasattr(locked, "updated_by"):
            locked.updated_by = bot
        locked.save(update_fields=["state", "updated_at"])

    logger.info(
        "agent_ticker: auto-paused issue=%s after cap hit",
        issue.pk,
    )
    return True


__all__ = [
    "DEFAULT_INTERVAL_SECONDS",
    "DEFAULT_MAX_TICKS",
    "DELEGATION_STATE_NAME",
    "INFINITE_MAX_TICKS",
    "PAUSED_STATE_NAME",
    "TRIGGER_COMMENT_AND_RUN",
    "TRIGGER_RUN_AI",
    "TRIGGER_TICK",
    "arm_ticker",
    "disarm_ticker",
    "dispatch_continuation_run",
    "dispatch_run_ai_run",
    "maybe_apply_deferred_pause",
    "maybe_disarm_on_terminal_signal",
    "reset_ticker_after_comment_and_run",
]
