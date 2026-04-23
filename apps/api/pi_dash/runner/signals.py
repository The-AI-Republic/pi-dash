# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Signal handlers for the runner app.

Currently wires one behavior:

- On workspace creation, auto-create a default pod named ``<workspace.name>-pod``
  so that runner registration and issue delegation work with zero setup. See
  ``.ai_design/issue_runner/design.md`` §7.1 and invariant #13.
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save
from django.dispatch import receiver

from pi_dash.db.models.workspace import Workspace
from pi_dash.runner.models import Pod

logger = logging.getLogger(__name__)


@receiver(post_save, sender=Workspace)
def create_default_pod_for_new_workspace(sender, instance: Workspace, created: bool, **kwargs):
    """When a workspace is created, ensure it has a default pod.

    Idempotent: no-op if the workspace already has any active pods (e.g. test
    fixtures that seed their own pod before the signal fires, or workspaces
    that existed before this signal was wired in).
    """
    if not created:
        return
    # Guard against fixtures / seed data that pre-populate pods.
    if Pod.objects.filter(workspace=instance).exists():
        return
    pod_name = f"{instance.name}-pod"
    try:
        Pod.objects.create(
            workspace=instance,
            name=pod_name,
            description="Auto-created default pod. Rename or add more pods anytime.",
            is_default=True,
            created_by=getattr(instance, "owner", None),
        )
    except Exception:
        # Don't block workspace creation if pod creation fails (e.g. a unique
        # constraint conflict from a manually-seeded pod racing us). Log and
        # let the ensure_workspace_pods management command catch up.
        logger.exception(
            "runner.signals: failed to auto-create default pod for workspace %s",
            instance.id,
        )
