# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import logging

from celery import shared_task

from pi_dash.core.platform_federation import (
    PlatformFederationError,
    platform_federation_enabled,
    reconcile_platform_org,
)
from pi_dash.db.models import PlatformFederationState, Workspace
from pi_dash.utils.exception_logger import log_exception

logger = logging.getLogger(__name__)


@shared_task
def reconcile_platform_federation() -> dict[str, int]:
    if not platform_federation_enabled():
        return {"workspaces": 0, "applied": 0, "skipped": 0, "failed": 0}

    counts = {"workspaces": 0, "applied": 0, "skipped": 0, "failed": 0}
    workspaces = Workspace.objects.exclude(platform_org_id__isnull=True).order_by("created_at")
    for workspace in workspaces:
        counts["workspaces"] += 1
        try:
            result = reconcile_platform_org(workspace.platform_org_id)
            counts["applied"] += result.get("applied", 0)
            counts["skipped"] += result.get("skipped", 0)
        except PlatformFederationError as exc:
            counts["failed"] += 1
            PlatformFederationState.objects.update_or_create(
                workspace=workspace,
                defaults={"status": PlatformFederationState.Status.ERROR, "last_error": str(exc)[:2000]},
            )
        except Exception as exc:
            counts["failed"] += 1
            log_exception(exc)
            PlatformFederationState.objects.update_or_create(
                workspace=workspace,
                defaults={"status": PlatformFederationState.Status.ERROR, "last_error": str(exc)[:2000]},
            )
    logger.info("Platform federation reconcile finished: %s", counts)
    return counts
