"""Executor-neutral, race-safe AgentRun terminalization and durable effects."""

from __future__ import annotations

import logging

from django.db import transaction
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import AgentRun, AgentRunEvent, AgentRunStatus

logger = logging.getLogger(__name__)
TERMINAL_STATUSES = {
    AgentRunStatus.COMPLETED,
    AgentRunStatus.FAILED,
    AgentRunStatus.CANCELLED,
    AgentRunStatus.BLOCKED,
    AgentRunStatus.REFUSED,
}


def finalize_agent_run(run_id, new_status, *, updates=None, expected_runner_id=None) -> bool:
    """First-writer-wins terminal transition for either executor."""
    if new_status not in TERMINAL_STATUSES:
        raise ValueError("new_status must be terminal")
    values = {
        "status": new_status,
        "ended_at": timezone.now(),
        "queue_position": None,
        "terminal_hooks_applied_at": None,
        "terminal_capacity_released_at": None,
        **(updates or {}),
    }
    with transaction.atomic():
        qs = AgentRun.objects.select_for_update().filter(pk=run_id).exclude(status__in=TERMINAL_STATUSES)
        if expected_runner_id is not None:
            qs = qs.filter(runner_id=expected_runner_id)
        run = qs.first()
        if run is None:
            return False
        AgentRun.objects.filter(pk=run.pk).update(**values)
        if (
            run.executor_kind == AgentExecutorKind.CLOUD_AGENT
            and not AgentRunEvent.objects.filter(agent_run=run, kind="terminal").exists()
        ):
            seq = (
                AgentRunEvent.objects.filter(agent_run=run).order_by("-seq").values_list("seq", flat=True).first() or 0
            ) + 1
            AgentRunEvent.objects.create(
                agent_run=run,
                seq=seq,
                kind="terminal",
                payload={"status": new_status, "error_code": values.get("error_code", "")},
            )
        transaction.on_commit(lambda: _publish_effects(run.pk))
    return True


def _publish_effects(run_id):
    from pi_dash.runner.tasks import apply_agent_run_terminal_effects

    try:
        apply_agent_run_terminal_effects.delay(str(run_id))
    except Exception:
        logger.exception("failed to publish terminal effects for run %s", run_id)
    # Preserve the established on-commit lifecycle contract while the queued
    # task and periodic reconciler close process-loss windows. A racing task is
    # harmless because the cursor rows are locked and idempotent.
    try:
        apply_terminal_effects(run_id)
    except Exception:
        logger.exception("failed to apply terminal effects for run %s", run_id)


def apply_terminal_effects(run_id) -> bool:
    """Apply orchestration hooks once, then deliver capacity release at least once."""
    from pi_dash.runner.services.run_lifecycle import (
        _apply_post_run_orchestration,
        _has_project_move_handoff,
        _post_failure_comment,
    )
    from pi_dash.runner.services.scheduler_hook import update_scheduler_binding_on_terminate

    pending_handoff = False
    with transaction.atomic():
        run = (
            AgentRun.objects.select_for_update(of=("self",))
            .select_related("work_item", "work_item__state", "work_item__project", "scheduler_binding")
            .filter(pk=run_id, status__in=TERMINAL_STATUSES)
            .first()
        )
        if run is None:
            return False
        if run.terminal_hooks_applied_at is None:
            has_handoff = _has_project_move_handoff(run)
            if run.status == AgentRunStatus.FAILED and not has_handoff:
                try:
                    # Keep this best-effort side effect behind a savepoint: a
                    # comment database failure must not poison the enclosing
                    # transaction or strand terminal capacity.
                    with transaction.atomic():
                        _post_failure_comment(run.id, run.error)
                except Exception:
                    logger.exception(
                        "run_lifecycle: failed to post failure comment for run %s",
                        run.id,
                    )
            if has_handoff:
                # A source-project run stopped for a move must not
                # disarm/pause the issue after it already belongs to the
                # target project; the handoff completion (below, after
                # this transaction to respect the issue → run lock order)
                # creates the target-project replacement instead.
                pending_handoff = True
            else:
                _apply_post_run_orchestration(run)
            if run.scheduler_binding_id:
                try:
                    with transaction.atomic():
                        update_scheduler_binding_on_terminate(run)
                except Exception:
                    logger.exception(
                        "scheduler.terminate_hook: failed for run %s",
                        run.id,
                    )
            run.terminal_hooks_applied_at = timezone.now()
            run.save(update_fields=["terminal_hooks_applied_at"])

    if pending_handoff:
        from pi_dash.orchestration.service import complete_project_move_handoff

        try:
            complete_project_move_handoff(run_id)
        except Exception:
            # The terminal transition is already committed; a handoff
            # recovery failure must not strand capacity release. The durable
            # marker stays on the run for reconciliation.
            logger.exception(
                "failed to complete project-move handoff for run %s",
                run_id,
            )

    run = AgentRun.objects.select_related("runner", "pod").get(pk=run_id)
    if run.terminal_capacity_released_at is None:
        if run.executor_kind == AgentExecutorKind.CLOUD_AGENT:
            from pi_dash.cloud_agent.dispatch import dispatch_waiting

            dispatch_waiting(run.workspace_id)
        else:
            from pi_dash.runner.services.matcher import drain_for_runner_by_id, drain_pod_by_id

            if run.runner_id:
                drain_for_runner_by_id(run.runner_id)
            if run.pod_id:
                drain_pod_by_id(run.pod_id)
        AgentRun.objects.filter(pk=run_id, terminal_capacity_released_at__isnull=True).update(
            terminal_capacity_released_at=timezone.now()
        )
    return True
