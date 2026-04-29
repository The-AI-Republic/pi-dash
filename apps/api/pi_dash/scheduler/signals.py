# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Signals for the project scheduler.

A ``post_save`` receiver on ``Workspace`` seeds the builtin scheduler
catalog when a new workspace is created. Combined with the migration
``0132_seed_builtin_schedulers``, this gives every workspace (existing
and future) the builtins without a startup-hook race.

See ``.ai_design/project_scheduler/design.md`` §6.6.
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save
from django.dispatch import receiver

from pi_dash.db.models.workspace import Workspace
from pi_dash.scheduler.builtins import ensure_builtin_schedulers

logger = logging.getLogger(__name__)


@receiver(post_save, sender=Workspace, dispatch_uid="scheduler.seed_builtins_on_workspace_create")
def _seed_builtins_on_workspace_create(sender, instance, created, **kwargs):
    if not created:
        return
    try:
        ensure_builtin_schedulers(instance)
    except Exception:
        # Don't block workspace creation if seeding hits an unexpected
        # error. The migration backfill will pick the row up on next deploy.
        logger.exception(
            "scheduler.seed_builtins: failed for new workspace %s", instance.pk
        )
