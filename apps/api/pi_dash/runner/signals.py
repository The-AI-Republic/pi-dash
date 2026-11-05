# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Signal handlers for the runner app.

On Project creation, auto-create one default ``Pod`` for the new project so
runner registration and issue dispatch work with zero setup. See
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§6.1.

The previous behaviour — auto-creating a workspace-default pod on Workspace
creation — is gone. Pods are now project-scoped (NOT NULL FK), and a
workspace-level pod would either crash at save time or silently re-introduce
the legacy model. The replacement is the per-project handler below.
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save, pre_delete
from django.dispatch import receiver

from pi_dash.db.models.project import Project
from pi_dash.runner.models import Pod, Runner

logger = logging.getLogger(__name__)


@receiver(post_save, sender=Project)
def create_default_pod_for_new_project(
    sender, instance: Project, created: bool, **kwargs
):
    """When a project is created, ensure it has a default pod.

    Idempotent: no-op if the project already has any active pod (e.g. test
    fixtures that seed their own pod, or projects backfilled by the 0007
    migration).
    """
    if not created:
        return
    # Guard against fixtures / seed data / migration backfill that
    # pre-populated a pod for this project.
    if Pod.objects.filter(project=instance).exists():
        return

    pod_name = f"{instance.identifier}_pod_1"
    try:
        Pod.objects.create(
            workspace_id=instance.workspace_id,
            project=instance,
            name=pod_name,
            description="Auto-created default pod. Add tier pods anytime.",
            is_default=True,
            created_by=getattr(instance, "project_lead", None)
            or getattr(instance, "default_assignee", None),
        )
    except Exception:
        # Don't block project creation if pod creation fails (e.g. a
        # unique-constraint conflict from a manually-seeded pod racing us).
        # Log; ensure_project_pods management command can retroactively fill.
        logger.exception(
            "runner.signals: failed to auto-create default pod for project %s",
            instance.id,
        )


@receiver(pre_delete, sender=Runner)
def detach_assigned_work_before_runner_delete(sender, instance: Runner, **kwargs):
    """Hard-deleting a runner must not silently turn strict runs into pod runs.

    ``AgentRun.pinned_runner`` and ``Issue.assigned_runner`` are ``SET_NULL``,
    so a bare delete would leave a strict run unpinned (free for any runner in
    the pod) and an issue pointing nowhere with no explanation. Every delete
    path goes through ``Runner.revoke`` first, which already does this with
    activity and terminal hooks; this handler is the backstop for one that
    does not (for example a cascade from a deleted workspace or user). It
    writes plain column updates only, so it is safe inside a cascade.
    Idempotent.
    """
    from django.utils import timezone

    from pi_dash.db.models.issue import Issue
    from pi_dash.runner.models import AgentRun, AgentRunStatus
    from pi_dash.runner.services.dispatch_rules import ASSIGNED_RUNNER_REMOVED

    AgentRun.objects.filter(pinned_runner=instance, status=AgentRunStatus.QUEUED, pin_strict=True).update(
        status=AgentRunStatus.FAILED,
        ended_at=timezone.now(),
        error_details={"code": ASSIGNED_RUNNER_REMOVED, "message": "The assigned runner was deleted."},
    )
    Issue.all_objects.filter(assigned_runner=instance).update(assigned_runner=None)
