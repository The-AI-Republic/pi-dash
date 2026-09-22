# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Make waiting on a blocker cheap (PDASHOSS01-198).

The agent still decides whether to wait for an open ``blocked_by`` target
(PDASHOSS01-195: inform and wake, never gate). When it waits it writes a
``Waiting on: PROJ-12, PROJ-13`` line in its workpad and yields
``waiting_on_external``. The platform honours that decision in two ways:

- **Pause** (:func:`waiting_pause`): ``fire_tick`` skips a *cadence* tick
  while the marker stands, without spending budget. The pause ends when a
  listed blocker closes (and the wake below fires), a human comments or
  moves the issue (a newer run or comment supersedes the waiting run), the
  marker is removed, or the project's ``agent_wait_max_pause_seconds``
  elapses.
- **Wake** (:func:`wake_dependents`): when an issue enters a ``completed`` /
  ``cancelled`` state, every dependent sitting in a ticking state with an
  armed clock gets a tick *now* (trigger ``blocker_completed``) instead of
  at its next cadence. A wake is a tick, never a state move: Backlog / Todo
  dependents are left alone.
"""

from __future__ import annotations

import logging
import time
from datetime import datetime, timedelta
from typing import List, Optional

from celery import shared_task
from django.db.models import Q
from django.utils import timezone

from pi_dash.db.models.issue import Issue, IssueActivity, IssueComment
from pi_dash.orchestration.blockers import CLOSED_STATE_GROUPS, blockers_queryset, dependents
from pi_dash.orchestration.workpad import parse_waiting_on

logger = logging.getLogger("pi_dash.worker")

#: ``fire_tick`` skip reason while the agent's marker holds the clock.
SKIP_WAITING_ON_BLOCKERS = "waiting-on-blockers"

#: Activity ``field`` recorded on a dependent that was woken.
WAKE_ACTIVITY_FIELD = "agent_wake"

#: ``done_payload.status`` values that mean the last run chose to wait:
#: the yield the blocker guidance asks for (``waiting_on_external``) and the
#: legacy done-fence ``noop``.
WAITING_RUN_STATUSES = frozenset({"waiting_on_external", "noop"})


def _identifier(issue: Issue) -> str:
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _resolve_identifiers(issue: Issue, identifiers: List[str]) -> List[Issue]:
    """Live work items in ``issue``'s workspace named by ``identifiers``."""
    match = Q(pk__in=[])
    for identifier in identifiers:
        project, _, sequence = identifier.rpartition("-")
        match |= Q(project__identifier__iexact=project, sequence_id=int(sequence))
    return list(Issue.issue_objects.filter(workspace_id=issue.workspace_id).filter(match))


def _pause_started_at(run) -> datetime:
    return run.ended_at or run.created_at


def waiting_pause(issue: Issue, *, now: Optional[datetime] = None) -> Optional[List[str]]:
    """The blockers a cadence tick on ``issue`` should keep waiting for.

    Returns the marker's identifiers when every pause condition holds, else
    ``None`` (tick as usual):

    - the latest run is finished and reported waiting
      (:data:`WAITING_RUN_STATUSES`) — any newer run (a human move, Comment
      & Run, Run AI, a wake) supersedes it;
    - the workpad carries a ``Waiting on:`` marker naming at least one item;
    - every named item is a live, still-open ``blocked_by`` target of the
      issue — an unknown ID, an unrelated issue, or one already
      completed / cancelled does not pause (nothing would wake it);
    - no human comment has landed since that run ended;
    - the project's max pause has not elapsed since that run ended.
    """
    from pi_dash.orchestration import service as orchestration_service

    now = now or timezone.now()
    run = orchestration_service._latest_prior_run(issue)
    if run is None or run.is_active:
        return None
    payload = run.done_payload if isinstance(run.done_payload, dict) else {}
    if payload.get("status") not in WAITING_RUN_STATUSES:
        return None

    identifiers = parse_waiting_on(issue.workpad or "")
    if not identifiers:
        return None

    started_at = _pause_started_at(run)
    max_pause = int(getattr(issue.project, "agent_wait_max_pause_seconds", 0) or 0)
    if max_pause <= 0:
        return None
    if now - started_at >= timedelta(seconds=max_pause):
        logger.info(
            "agent_ticker.fire_tick: resume issue=%s reason=max-pause-elapsed waiting_on=%s",
            issue.pk,
            ",".join(identifiers),
        )
        return None

    human_comment_since = (
        IssueComment.objects.filter(
            issue_id=issue.pk,
            created_at__gt=started_at,
            speaker_type=IssueComment.SpeakerType.HUMAN,
        )
        .exclude(actor__is_bot=True)
        .exists()
    )
    if human_comment_since:
        return None

    listed = _resolve_identifiers(issue, identifiers)
    if len(listed) != len(identifiers):
        return None
    open_blocker_ids = set(
        blockers_queryset(issue)
        .exclude(state__group__in=CLOSED_STATE_GROUPS)
        .filter(pk__in=[i.pk for i in listed])
        .values_list("pk", flat=True)
    )
    if len(open_blocker_ids) != len(listed):
        return None
    return identifiers


def _record_wake_activity(dependent: Issue, blocker: Issue) -> None:
    from pi_dash.orchestration.workpad import get_agent_system_user

    group = blocker.state.group if blocker.state else "completed"
    identifier = _identifier(blocker)
    IssueActivity.objects.create(
        issue=dependent,
        project_id=dependent.project_id,
        workspace_id=dependent.workspace_id,
        verb="updated",
        field=WAKE_ACTIVITY_FIELD,
        old_value=group,
        new_value=identifier,
        comment=f"Woken: {identifier} {group}",
        actor=get_agent_system_user(),
        epoch=time.time(),
    )


def wake_dependent(dependent: Issue, blocker: Issue) -> bool:
    """Fire an immediate ``blocker_completed`` tick on ``dependent``.

    Only a dependent in a ticking state with an armed clock is woken; the
    usual ``fire_tick`` guards (active run, cap, pending entry) still apply.
    Returns whether a run was dispatched.
    """
    from pi_dash.bgtasks.agent_ticker import fire_tick
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
    from pi_dash.orchestration.agent_phases import is_ticking_state
    from pi_dash.orchestration.scheduling import TRIGGER_BLOCKER_COMPLETED

    if not is_ticking_state(dependent.state):
        logger.info(
            "agent_wake: skip dependent=%s blocker=%s reason=not-ticking-state",
            dependent.pk,
            blocker.pk,
        )
        return False
    ticker_id = (
        IssueAgentTicker.objects.filter(issue_id=dependent.pk, enabled=True).values_list("pk", flat=True).first()
    )
    if ticker_id is None:
        logger.info(
            "agent_wake: skip dependent=%s blocker=%s reason=ticker-not-armed",
            dependent.pk,
            blocker.pk,
        )
        return False
    fired = fire_tick(str(ticker_id), trigger=TRIGGER_BLOCKER_COMPLETED)
    if fired:
        _record_wake_activity(dependent, blocker)
    logger.info(
        "agent_wake: dependent=%s blocker=%s fired=%s",
        dependent.pk,
        blocker.pk,
        fired,
    )
    return fired


@shared_task(name="pi_dash.orchestration.wake.wake_dependents")
def wake_dependents(blocker_id: str) -> int:
    """Wake every waiting dependent of a blocker that just closed.

    Enqueued from the ``Issue`` post-save hook on a transition *into* a
    ``completed`` / ``cancelled`` group (so re-saving a closed issue never
    re-fires). Re-checks the blocker is still closed when the task runs.
    Returns the number of dependents that got a run.
    """
    blocker = Issue.issue_objects.select_related("state", "project").filter(pk=blocker_id).first()
    if blocker is None or blocker.state is None or blocker.state.group not in CLOSED_STATE_GROUPS:
        return 0
    woken = 0
    for dependent in dependents(blocker):
        try:
            if wake_dependent(dependent, blocker):
                woken += 1
        except Exception:  # noqa: BLE001 — one bad dependent must not strand the rest
            logger.exception("agent_wake: failed dependent=%s blocker=%s", dependent.pk, blocker.pk)
    return woken
