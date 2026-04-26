# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt composition.

`composer.build_first_turn(issue, run)` is the only caller-facing entrypoint.
Views and orchestration never render directly.
"""

from __future__ import annotations

from typing import Optional

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.workspace import Workspace
from pi_dash.prompting.context import build_context
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.renderer import render
from pi_dash.runner.models import AgentRun


class PromptTemplateNotFound(Exception):
    """No active PromptTemplate (workspace-scoped or global) was available."""


def load_template(
    workspace: Optional[Workspace],
    name: str = PromptTemplate.DEFAULT_NAME,
) -> PromptTemplate:
    """Return the active template for ``(workspace, name)`` or fall back to the
    global (``workspace IS NULL``) row. Raises if neither exists.
    """
    if workspace is not None:
        ws_match = (
            PromptTemplate.objects.filter(
                workspace=workspace, name=name, is_active=True
            )
            .order_by("-updated_at")
            .first()
        )
        if ws_match is not None:
            return ws_match

    global_match = (
        PromptTemplate.objects.filter(
            workspace__isnull=True, name=name, is_active=True
        )
        .order_by("-updated_at")
        .first()
    )
    if global_match is None:
        raise PromptTemplateNotFound(
            f"No active PromptTemplate for name={name!r}; "
            "did the seed migration run?"
        )
    return global_match


def build_first_turn(issue: Issue, run: AgentRun) -> str:
    """Render the first-turn prompt for ``run`` executing ``issue``."""
    template = load_template(issue.workspace)
    context = build_context(issue, run)
    return render(template.body, context)


def build_continuation(issue: Issue, run: AgentRun) -> str:
    """Render a continuation prompt for a follow-up run.

    The agent reattaches to its prior session via native resume
    (``--resume`` / ``thread/resume``), so it already has the
    conversation context in memory. The prompt for the follow-up turn
    is therefore just the human's new input — concatenated comments
    written by non-bot users since the parent run started.

    Falls back to the full first-turn template when there is no parent
    run with a ``started_at`` to anchor the comment sweep against
    (e.g. parent crashed before reporting a session id).
    """
    from pi_dash.db.models.issue import IssueComment

    parent = run.parent_run
    if parent is None or parent.started_at is None:
        return build_first_turn(issue, run)
    comments = list(
        IssueComment.objects.filter(
            issue=issue,
            actor__is_bot=False,
            created_at__gt=parent.started_at,
        )
        .order_by("created_at")
        .values_list("comment_stripped", flat=True)
    )
    bodies = [c.strip() for c in comments if c and c.strip()]
    if not bodies:
        return "(continuation requested with no new human input — proceed)"
    return "\n\n---\n\n".join(bodies)
