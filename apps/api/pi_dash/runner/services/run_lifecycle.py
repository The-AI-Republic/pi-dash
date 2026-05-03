# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared cloud-side handlers for runner→cloud run-lifecycle frames.

Extracted from the legacy ``RunnerConsumer`` so the new HTTP endpoints
(``run_endpoints.py``) can reuse the same orchestration: pause →
IssueComment + deferred-pause + drain re-fire; ``resume_unavailable``
fail → re-queue + pin-drop + parent thread_id clear + drain.

These two paths carry side-effects beyond a status update; without
them the agent's question-for-human never reaches the user thread and
runs that miss their session on disk fail-stop instead of recovering.
"""

from __future__ import annotations

import logging
from typing import Any, Dict
from uuid import UUID

from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
)

logger = logging.getLogger(__name__)


def apply_run_paused(
    runner: Runner, run_id: UUID | str, payload: Dict[str, Any]
) -> None:
    """Mark run paused, post the agent's question to the issue thread,
    apply deferred-pause workspace transitions, and re-fire drain.

    See legacy ``RunnerConsumer._handle_run_paused``.
    """
    AgentRun.objects.filter(id=run_id, runner=runner).update(
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        done_payload=payload,
    )
    try:
        run = AgentRun.objects.select_related("work_item").get(id=run_id)
    except AgentRun.DoesNotExist:
        return

    if run.work_item_id is not None:
        from django.utils.html import format_html

        from pi_dash.db.models.issue import IssueComment
        from pi_dash.orchestration.workpad import get_agent_system_user

        question = (payload.get("autonomy") or {}).get("question_for_human")
        summary = payload.get("summary")
        body_parts: list[str] = []
        if question:
            body_parts.append(
                format_html(
                    "<p><strong>Agent paused — question:</strong></p><p>{}</p>",
                    question,
                )
            )
        if summary:
            body_parts.append(
                format_html("<p><em>Summary so far:</em> {}</p>", summary)
            )
        if body_parts:
            IssueComment.objects.create(
                issue=run.work_item,
                project=run.work_item.project,
                workspace=run.work_item.workspace,
                actor=get_agent_system_user(),
                comment_html="".join(body_parts),
            )

    from pi_dash.orchestration.scheduling import maybe_apply_deferred_pause
    from pi_dash.runner.services.matcher import drain_for_runner_by_id

    def _pause_and_drain(rid=run_id, runner_id=runner.id):
        paused = (
            AgentRun.objects.select_related(
                "work_item",
                "work_item__state",
                "work_item__project",
            )
            .filter(pk=rid)
            .first()
        )
        if paused is not None:
            try:
                maybe_apply_deferred_pause(paused)
            except Exception:
                logger.exception(
                    "orchestration.error: deferred-pause failed for run %s",
                    rid,
                )
        drain_for_runner_by_id(runner_id)

    transaction.on_commit(_pause_and_drain)


def apply_run_resume_unavailable(runner: Runner, run_id: UUID | str) -> None:
    """Re-queue a run whose pinned session disappeared.

    Drops the runner / pin / assigned_at and clears the parent's
    ``thread_id`` so the next dispatch builds a fresh-session Assign.
    See legacy ``RunnerConsumer._handle_resume_unavailable``.
    """
    run = AgentRun.objects.filter(id=run_id, runner=runner).first()
    if run is None:
        return
    run.status = AgentRunStatus.QUEUED
    run.runner = None
    run.pinned_runner = None
    run.assigned_at = None
    if run.parent_run is not None and run.parent_run.thread_id:
        run.parent_run.thread_id = ""
        run.parent_run.save(update_fields=["thread_id"])
    run.save(
        update_fields=["status", "runner", "pinned_runner", "assigned_at"]
    )

    from pi_dash.runner.services.matcher import drain_pod_by_id

    if run.pod_id is not None:
        transaction.on_commit(
            lambda pid=run.pod_id: drain_pod_by_id(pid)
        )


def _post_failure_comment(run_id: UUID | str, error_detail: str) -> None:
    """Post a single IssueComment from the agent system user describing
    why a run failed. Mirrors the shape of ``apply_run_paused``'s comment
    creation so the issue activity feed is the one place a user has to
    look to understand what happened.

    Conservative wording — we surface the runner's classification + any
    detail it sent (last command, stderr tail) rather than asserting a
    root cause we can't prove from telemetry alone. Hidden behind the
    public ``finalize_run_terminal`` entry-point so callers don't have
    to opt in.
    """
    from django.utils.html import escape, format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user

    run = (
        AgentRun.objects.select_related("work_item", "work_item__project")
        .filter(pk=run_id)
        .first()
    )
    if run is None or run.work_item_id is None:
        return

    detail = (error_detail or "").strip()
    if detail:
        body = format_html(
            "<p><strong>Run failed.</strong></p><pre>{}</pre>",
            detail,
        )
    else:
        # Defensive: we'd rather post "(no diagnostic detail)" than
        # silently drop the activity entry.
        body = format_html(
            "<p><strong>Run failed.</strong> {}</p>",
            escape("(no diagnostic detail)"),
        )

    IssueComment.objects.create(
        issue=run.work_item,
        project=run.work_item.project,
        workspace=run.work_item.workspace,
        actor=get_agent_system_user(),
        comment_html=body,
    )


def finalize_run_terminal(
    runner: Runner,
    run_id: UUID | str,
    new_status: AgentRunStatus,
    *,
    done_payload: Any = None,
    error_detail: str = "",
) -> None:
    """Stamp a terminal status and re-fire drain after commit.

    Used by the COMPLETED / FAILED / CANCELLED endpoints. The
    ``resume_unavailable`` special case is handled in
    :func:`apply_run_resume_unavailable`; callers should branch on
    that BEFORE calling this helper.
    """
    updates: Dict[str, Any] = {
        "status": new_status,
        "ended_at": timezone.now(),
    }
    if new_status == AgentRunStatus.COMPLETED:
        updates["done_payload"] = done_payload
    if new_status == AgentRunStatus.FAILED and error_detail:
        updates["error"] = error_detail[:16000]
    AgentRun.objects.filter(id=run_id, runner=runner).update(**updates)

    if new_status == AgentRunStatus.FAILED:
        try:
            _post_failure_comment(run_id, error_detail)
        except Exception:
            # Comment posting must never block the lifecycle terminal
            # transition. Log and move on so the run still gets reaped /
            # the pod still gets drained.
            logger.exception(
                "run_lifecycle: failed to post failure comment for run %s",
                run_id,
            )

    from pi_dash.orchestration.scheduling import maybe_apply_deferred_pause
    from pi_dash.runner.services.matcher import (
        drain_for_runner_by_id,
        drain_pod_by_id,
    )
    from pi_dash.runner.services.scheduler_hook import (
        update_scheduler_binding_on_terminate,
    )

    runner_id = runner.id
    pod_id = runner.pod_id

    def _pause_and_drain(rid=run_id, rnr=runner_id, pid=pod_id):
        run = (
            AgentRun.objects.select_related(
                "work_item",
                "work_item__state",
                "work_item__project",
                "scheduler_binding",
            )
            .filter(pk=rid)
            .first()
        )
        if run is not None:
            try:
                maybe_apply_deferred_pause(run)
            except Exception:
                logger.exception(
                    "orchestration.error: deferred-pause failed for run %s", rid
                )
            if run.scheduler_binding_id is not None:
                try:
                    update_scheduler_binding_on_terminate(run)
                except Exception:
                    logger.exception(
                        "scheduler.terminate_hook: failed for run %s", rid
                    )
        drain_for_runner_by_id(rnr)
        if pid is not None:
            drain_pod_by_id(pid)

    transaction.on_commit(_pause_and_drain)
