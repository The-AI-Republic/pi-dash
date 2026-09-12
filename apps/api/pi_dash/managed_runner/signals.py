# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Structured observability events for managed-runner run creation.

Two of the events in design §15.1 — ``managed_runner.run_pinned`` and
``managed_runner.queued_waiting`` — describe a run the moment it is created, but
the pinning decision is made in ``cloud_agent/creation.py`` *before* the row
exists, and the fields it returns are splatted into ``AgentRun.objects.create``
by five separate creation sites. A single ``post_save`` handler is the one place
that sees both the run id and the resolved runner together, for every path,
without threading a log call through each caller.

Every managed run that is admitted carries a ``pinned_runner`` (an online one
when available, the enrolled-but-offline one when an automatic run is parked to
wait). The two cases are told apart by ``error_code``: a parked run carries
``desktop_not_connected`` (``ManagedRunnerReason.NOT_CONNECTED``); a run pinned
to a live desktop carries none.
"""

from __future__ import annotations

import logging

from django.db.models.signals import post_save
from django.dispatch import receiver

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.runner.models import AgentRun

logger = logging.getLogger(__name__)


@receiver(post_save, sender=AgentRun)
def log_managed_run_pinning(sender, instance: AgentRun, created: bool, **kwargs):
    """Emit ``run_pinned`` / ``queued_waiting`` for a newly created managed run.

    A no-op for every other executor kind and for updates, so the cost on the
    hot run-creation path is a single attribute comparison.
    """
    if not created or instance.executor_kind != AgentExecutorKind.MANAGED_RUNNER:
        return
    if instance.error_code == ManagedRunnerReason.NOT_CONNECTED:
        logger.info(
            "managed_runner.queued_waiting run=%s runner=%s",
            instance.id,
            instance.pinned_runner_id,
        )
    elif instance.pinned_runner_id is not None:
        logger.info(
            "managed_runner.run_pinned run=%s runner=%s",
            instance.id,
            instance.pinned_runner_id,
        )
