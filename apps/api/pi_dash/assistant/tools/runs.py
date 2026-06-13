# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Coding-run tools: read run status and dispatch a coding agent run."""

from __future__ import annotations

from typing import Optional

from pydantic_ai import ModelRetry, RunContext

from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.tools import _results, _scoping


@assistant.tool
def get_run_status(ctx: RunContext[AssistantDeps], issue_id: str) -> dict:
    """Get the status of coding agent runs for an issue."""
    issue = _scoping.get_issue(ctx.deps, issue_id)
    from pi_dash.runner.models import AgentRun

    runs = AgentRun.objects.filter(work_item_id=issue.id).order_by("-created_at")[:5]
    out = []
    for r in runs:
        out.append(
            {
                "id": str(r.id),
                "status": r.status,
                "created_at": r.created_at.isoformat() if r.created_at else None,
            }
        )
    return {"issue_id": str(issue.id), "runs": out}


@assistant.tool
def dispatch_coding_run(
    ctx: RunContext[AssistantDeps],
    issue_id: str,
    target_state_id: Optional[str] = None,
) -> dict:
    """Start a coding agent run on an issue by moving it into a delegated state.
    Requires write access. Identical to dragging the issue into that state in the UI."""
    deps = ctx.deps
    issue = _scoping.get_issue(deps, issue_id)
    _scoping.require_project_write(deps, str(issue.project_id))

    from pi_dash.orchestration import service as orchestration
    from pi_dash.orchestration.agent_phases import PHASES, is_ticking_state

    states = _scoping.project_states(deps, str(issue.project_id))
    if target_state_id:
        target = states.filter(id=target_state_id).first()
        if target is None:
            raise ModelRetry("That state is not valid for this project.")
        if not is_ticking_state(target):
            raise ModelRetry(
                "That state does not start a coding run. Pick a delegated state "
                "such as 'In Progress' or 'In Review'."
            )
    else:
        target = (
            states.filter(group__in=list(PHASES.keys())).order_by("sequence").first()
        )
        if target is None:
            return {
                "dispatched": False,
                "error": "no_delegation_state",
                "detail": "This project has no state that triggers a coding run.",
            }

    from_state = issue.state
    if from_state and from_state.id == target.id:
        return {
            "dispatched": False,
            "error": "already_in_state",
            "detail": f"Issue is already in '{target.name}'.",
        }

    outcome = orchestration.handle_issue_state_transition(
        issue, from_state, target, actor=_scoping.user_for(deps), dispatch_immediate=True
    )
    run_id = str(outcome.created_run.id) if outcome.created_run is not None else None
    _results.record_write(
        deps,
        f"Started a coding run on {issue.project.identifier}-{issue.sequence_id} "
        f"(moved to '{target.name}')",
        links=[_results.issue_link(deps, issue)],
    )
    return {
        "dispatched": run_id is not None,
        "run_id": run_id,
        "new_state": target.name,
        "reason": outcome.reason,
    }
