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

from django.db.models.signals import post_save
from django.dispatch import receiver

from pi_dash.db.models.project import Project
from pi_dash.runner.models import Pod

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
