# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Write-side rule for an issue's executor.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §6 (revision
4.0). Shared by the web-app and external issue serializers so both enforce
the same rule.

Runner (``local_runner``) is the only executor an issue can be set to; which
machine runs it is answered by the issue's pod (§7). Older clients still send
``managed_runner`` or ``cloud_agent``; both are stored as ``local_runner``,
pod-level: the issue keeps its ``assigned_pod`` and no runner is named.
Dedicating an issue to one runner belongs to the dedicated-runner design
(HA §23) and is not part of this rule.
"""

from __future__ import annotations

from rest_framework import serializers

from pi_dash.core.agent_execution import AgentExecutorKind, normalize_executor_write


def normalize_issue_executor(attrs: dict, *, instance) -> None:
    """Translate and validate ``attrs["agent_executor"]`` in place.

    ``None`` clears the override (inherit the project default). A change of
    executor under a live run is refused, as before: ``AgentRun.executor_kind``
    is snapshotted at creation, so switching under a live run would silently
    apply only to the next dispatch. Raises ``serializers.ValidationError``.
    """
    if "agent_executor" not in attrs:
        return
    executor = attrs["agent_executor"]
    if executor is not None and executor not in AgentExecutorKind.values:
        raise serializers.ValidationError({"agent_executor": "unknown agent executor"})
    attrs["agent_executor"] = normalize_executor_write(executor)
    if (
        instance is not None
        and str(attrs["agent_executor"]) != str(instance.agent_executor)
        and instance.has_active_run
    ):
        raise serializers.ValidationError(
            {"agent_executor": "cannot change the execution target while the issue has an active run"}
        )
