# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Write-side rules for an issue's execution target.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §6 and §7.
Shared by the web-app and external issue serializers so both enforce the
same rules:

- **Executor.** Runner (``local_runner``) is the only executor an issue can
  be set to. Older clients still send ``managed_runner`` (translated to
  ``local_runner`` plus ``assigned_runner`` = the caller's engine runner for
  the project, which is the old behaviour) or ``cloud_agent`` (translated to
  ``local_runner``, pod-level).
- **Assigned runner.** Only the runner's owner can assign an issue to it, and
  only a live runner in the issue's project. Setting it sets
  ``assigned_pod`` to the runner's pod; choosing a different pod clears it.
- **Mid-flight.** The assignment may change while the issue only has a
  *queued* run (that run is re-pinned in the same transaction, see
  :func:`pi_dash.runner.services.dispatch_rules.repin_queued_runs_for_issue`),
  not while a run is executing.
"""

from __future__ import annotations

from typing import Optional

from rest_framework import serializers

from pi_dash.core.agent_execution import AgentExecutorKind

#: Statuses of an active run that is not merely queued. While one exists the
#: issue's pod or runner cannot change: the run's pod is fixed.
def _non_queued_active_statuses():
    from pi_dash.runner.models import AgentRunStatus
    from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES

    return [s for s in NON_TERMINAL_STATUSES if s != AgentRunStatus.QUEUED]


def issue_has_non_queued_active_run(issue) -> bool:
    from pi_dash.runner.models import AgentRun

    return AgentRun.objects.filter(work_item=issue, status__in=_non_queued_active_statuses()).exists()


def callers_engine_runner(project, user):
    """The caller's engine runner for ``project``: online first, else the most
    recently seen live one, else None. Backs the ``managed_runner`` write
    translation for older desktop builds."""
    from pi_dash.managed_runner.policy import enrolled_managed_runners

    if user is None or not getattr(user, "is_authenticated", False):
        return None
    enrolled = enrolled_managed_runners(project, user).select_related("pod")
    return enrolled.assignable().order_by("-last_heartbeat_at").first() or enrolled.order_by(
        "-last_heartbeat_at", "-created_at"
    ).first()


def normalize_issue_dispatch_target(
    attrs: dict,
    *,
    instance,
    project,
    user,
    pod_key: str = "assigned_pod",
    pod_error_key: str = "assigned_pod_id",
    runner_error_key: str = "assigned_runner_id",
    allow_runner_write: bool = True,
) -> bool:
    """Apply the executor / assigned-runner / pod rules to ``attrs`` in place.

    Returns True when the write changes the issue's assigned runner, so the
    caller can re-pin queued runs after saving. Raises
    ``serializers.ValidationError`` with field-keyed messages.
    """
    if "agent_executor" in attrs:
        executor = attrs["agent_executor"]
        if executor is not None and executor not in AgentExecutorKind.values:
            raise serializers.ValidationError({"agent_executor": "unknown agent executor"})
        if executor == AgentExecutorKind.MANAGED_RUNNER:
            attrs["agent_executor"] = AgentExecutorKind.LOCAL_RUNNER
            if "assigned_runner" not in attrs and project is not None:
                engine = callers_engine_runner(project, user)
                if engine is not None:
                    attrs["assigned_runner"] = engine
        elif executor == AgentExecutorKind.CLOUD_AGENT:
            attrs["agent_executor"] = AgentExecutorKind.LOCAL_RUNNER
        if (
            instance is not None
            and str(attrs["agent_executor"]) != str(instance.agent_executor)
            and instance.has_active_run
        ):
            raise serializers.ValidationError(
                {"agent_executor": "cannot change the execution target while the issue has an active run"}
            )

    if "assigned_runner" in attrs and not allow_runner_write:
        attrs.pop("assigned_runner")

    if "assigned_runner" in attrs:
        runner = attrs["assigned_runner"]
        if runner is not None:
            _validate_assignable_runner(runner, project=project, user=user, error_key=runner_error_key)
            # The invariant: an issue with an assigned runner is dispatched
            # in that runner's current pod.
            attrs[pod_key] = runner.pod
    elif pod_key in attrs and instance is not None and instance.assigned_runner_id is not None:
        # Choosing a pod explicitly makes the issue pod-level. Re-sending the
        # current pod (the web form re-PATCHes the full set) changes nothing.
        pod = attrs[pod_key]
        new_pod_id = pod.id if pod is not None else None
        if str(new_pod_id) != str(instance.assigned_pod_id):
            attrs["assigned_runner"] = None

    changed = False
    if "assigned_runner" in attrs:
        new_runner_id = getattr(attrs["assigned_runner"], "id", None)
        old_runner_id = getattr(instance, "assigned_runner_id", None) if instance is not None else None
        changed = str(new_runner_id) != str(old_runner_id)
        if changed and instance is not None and issue_has_non_queued_active_run(instance):
            raise serializers.ValidationError(
                {runner_error_key: "cannot change the issue's runner while a run is in progress"}
            )
    return changed


def _validate_assignable_runner(runner, *, project, user, error_key: str) -> None:
    user_id = getattr(user, "id", None) if user is not None and getattr(user, "is_authenticated", False) else None
    if user_id is None or runner.owner_id != user_id:
        # Only the owner can point an issue at their runner: the runner's
        # usage rule and wallet are theirs (design §7, §8).
        raise serializers.ValidationError({error_key: "you can only assign an issue to one of your own runners"})
    if runner.is_revoked:
        raise serializers.ValidationError({error_key: "runner has been removed"})
    project_id = getattr(project, "id", project)
    if runner.pod_id is None or project_id is None or str(runner.pod.project_id) != str(project_id):
        raise serializers.ValidationError({error_key: "runner is in a different project"})


def project_for_issue_write(serializer, attrs) -> Optional[object]:
    """The project an issue write applies to (instance, attrs or context)."""
    if serializer.instance is not None:
        return serializer.instance.project
    project = attrs.get("project")
    if project is not None:
        return project
    project_id = serializer.context.get("project_id")
    if project_id is None:
        return None
    from pi_dash.db.models import Project

    return Project.objects.filter(pk=project_id).first()
