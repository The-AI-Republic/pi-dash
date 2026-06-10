# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Scheduler-binding bookkeeping after a run reaches a terminal state.

Extracted from the legacy ``RunnerConsumer`` so the new HTTP run
endpoints can call it without depending on Channels.
"""

from __future__ import annotations

from pi_dash.runner.models import AgentRun, AgentRunStatus


def update_scheduler_binding_on_terminate(run: AgentRun) -> None:
    """Update ``SchedulerBinding.last_error`` on terminal run state.

    See ``.ai_design/project_scheduler/design.md`` §6.5.
    """
    binding = run.scheduler_binding
    if binding is None:
        return
    from pi_dash.db.models.scheduler import LAST_ERROR_MAX_LEN

    if run.status == AgentRunStatus.COMPLETED:
        if binding.last_error:
            binding.last_error = ""
            binding.save(update_fields=["last_error", "updated_at"])
    elif run.status in (AgentRunStatus.FAILED, AgentRunStatus.CANCELLED, AgentRunStatus.REFUSED):
        if run.status == AgentRunStatus.REFUSED:
            # Surface the decline category so the scheduler tab shows *why* the
            # tick produced nothing (e.g. "refused (cyber)"), not a bare error.
            raw = f"refused ({run.refusal_category or 'unknown'})"
            if run.error:
                raw = f"{raw}: {run.error}"
        else:
            raw = run.error or run.status
        msg = raw[:LAST_ERROR_MAX_LEN]
        if binding.last_error != msg:
            binding.last_error = msg
            binding.save(update_fields=["last_error", "updated_at"])
