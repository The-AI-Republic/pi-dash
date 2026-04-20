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
    """Continuation-turn rendering is explicitly deferred in MVP.

    The runner is single-turn today; when we add multi-turn we will compose
    continuations from workpad state plus the prior turn transcript.
    """
    raise NotImplementedError(
        "Continuation turns are not supported in MVP; see "
        ".ai_design/prompt_system/prompt-system-design.md §2 decision 7."
    )
