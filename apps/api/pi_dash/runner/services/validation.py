# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pre-create validation shared by the direct run-creation endpoint and the
orchestration path.

See ``.ai_design/issue_runner/design.md`` §6.5.

The goal is to reject a run creation *before* the DB insert when any of:

- the caller is not a member of the target workspace,
- the referenced work item doesn't belong to the workspace,
- the resolved pod doesn't belong to the workspace or is soft-deleted,
- no pod can be resolved at all.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from pi_dash.db.models.issue import Issue
from pi_dash.runner.models import Pod
from pi_dash.runner.services.permissions import is_workspace_member


class RunCreationError(Exception):
    """Raised when pre-create validation fails. Carries an HTTP-ish status."""

    def __init__(self, status: int, message: str, code: str = ""):
        super().__init__(message)
        self.status = status
        self.message = message
        self.code = code


@dataclass(frozen=True)
class ValidatedRunContext:
    """The verified inputs for an ``AgentRun`` row about to be inserted."""

    workspace_id: object
    work_item_id: Optional[object]
    pod: Pod
    created_by: object  # User instance


def validate_run_creation(
    user,
    workspace_id,
    *,
    work_item_id=None,
    pod_id=None,
) -> ValidatedRunContext:
    """Run the §6.5 checks and return a :class:`ValidatedRunContext`.

    Parameters are intentionally keyword-only for the optional fields so call
    sites can't accidentally swap work_item_id / pod_id.

    Raises
    ------
    RunCreationError
        With ``status=403`` if the caller is not a workspace member.
        With ``status=400`` for consistency violations.
        With ``status=409`` if no pod can be resolved.
    """
    if not workspace_id:
        raise RunCreationError(400, "workspace is required", "workspace_required")

    # 1. Workspace membership.
    if not is_workspace_member(user, workspace_id):
        raise RunCreationError(
            403,
            "caller is not a member of the target workspace",
            "not_workspace_member",
        )

    # 2. Work-item consistency.
    if work_item_id is not None:
        issue = (
            Issue.objects.filter(pk=work_item_id)
            .values("workspace_id", "assigned_pod_id", "project_id")
            .first()
        )
        if issue is None:
            raise RunCreationError(400, "work_item does not exist", "work_item_missing")
        if str(issue["workspace_id"]) != str(workspace_id):
            raise RunCreationError(
                400,
                "work_item does not belong to workspace",
                "work_item_workspace_mismatch",
            )
    else:
        issue = None

    # 3. Pod resolution and consistency. Project comes from the issue
    # if available; for run-creation calls without a work_item we don't
    # have a project to anchor to, so the pod_id (if any) must be
    # explicit and we cannot fall back to a default.
    project_id = (issue or {}).get("project_id") if issue else None
    pod = _resolve_pod(
        workspace_id=workspace_id,
        project_id=project_id,
        pod_id=pod_id,
        issue_assigned_pod_id=(issue or {}).get("assigned_pod_id") if issue else None,
    )

    return ValidatedRunContext(
        workspace_id=workspace_id,
        work_item_id=work_item_id,
        pod=pod,
        created_by=user,
    )


def _resolve_pod(
    *, workspace_id, project_id, pod_id, issue_assigned_pod_id
) -> Pod:
    """Resolve the pod the run belongs to.

    Priority: explicit pod_id > issue.assigned_pod > project.default_pod.

    Workspace consistency is still enforced (a pod must be in the request's
    workspace) but the project-equality check is the stronger gate when a
    project is known: a Project P run cannot land in a Project Q pod even
    if both are in the same workspace.
    """
    # Explicit pod: must exist, not be soft-deleted, and belong to the
    # workspace and the issue's project (when known).
    if pod_id is not None:
        pod = Pod.objects.filter(pk=pod_id).first()
        if pod is None:
            raise RunCreationError(400, "pod does not exist or has been deleted", "pod_missing")
        if str(pod.workspace_id) != str(workspace_id):
            raise RunCreationError(
                400, "pod does not belong to workspace", "pod_workspace_mismatch"
            )
        if project_id is not None and str(pod.project_id) != str(project_id):
            raise RunCreationError(
                400,
                "pod does not belong to issue's project",
                "pod_project_mismatch",
            )
        return pod

    # Fall back to the issue's pinned pod if one exists, with the same
    # project-equality check.
    if issue_assigned_pod_id is not None:
        pod = Pod.objects.filter(pk=issue_assigned_pod_id).first()
        if (
            pod is not None
            and str(pod.workspace_id) == str(workspace_id)
            and (project_id is None or str(pod.project_id) == str(project_id))
        ):
            return pod
        # Pinned pod was soft-deleted or stale — fall through.

    # Final fallback: project's default pod. Without a project we can't
    # resolve a pod (the previous workspace-default fallback is gone — see
    # .ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md §8).
    if project_id is not None:
        default = Pod.default_for_project_id(project_id)
        if default is not None:
            return default

    raise RunCreationError(
        409,
        "no pod available; ensure the issue has a project with a default pod",
        "no_pod_available",
    )
