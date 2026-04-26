# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Template context builder.

The *only* data a Jinja template ever sees. Ordinary ORM objects must not leak
into `renderer.render()` — the sandboxed environment has very little ability to
defend against unintended attribute access on them.
"""

from __future__ import annotations

from typing import Any, Dict

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import AgentRun


def _issue_description_markdown(issue: Issue) -> str:
    """Return markdown-ish text for the agent to read.

    Issue descriptions in Pi Dash are stored as rich text (JSON + HTML). The
    handbook promises the agent a raw-markdown blob; until the full JSON->md
    conversion lands we fall back to the plain-text (`description_stripped`)
    representation, which preserves line breaks and code fences agents rely on.
    """
    if issue.description_stripped:
        return str(issue.description_stripped)
    return ""


def _absolute_issue_url(issue: Issue) -> str:
    """Return a best-effort deep link. Full URL construction lives in the
    web layer; we return a relative path so templates still have something
    useful to show."""
    ws = getattr(issue.workspace, "slug", "")
    proj = getattr(issue.project, "identifier", "")
    return f"/{ws}/projects/{issue.project_id}/issues/{issue.id}" if ws else ""


def build_context(issue: Issue, run: AgentRun) -> Dict[str, Any]:
    """Build the dict passed into Jinja.

    Never raises on missing optional fields — empty strings, empty lists, and
    ``None`` are fine; templates handle absence with ``{% if %}``.
    """

    project = issue.project
    workspace = issue.workspace
    state = getattr(issue, "state", None)
    parent = issue.parent

    # Plain M2M traversals; if these raise it's a real ORM error and should
    # bubble up to the caller (which already wraps rendering in PromptRenderError
    # at the composer layer).
    labels = list(issue.labels.all().values_list("name", flat=True))
    assignees = [
        (u.display_name or u.email or "") for u in issue.assignees.all()
    ]
    project_states = [
        {
            "name": s.name,
            "group": s.group,
            "description": s.description or "",
        }
        for s in State.objects.filter(project=project)
    ]

    attempt = _compute_attempt(issue, run)

    return {
        "issue": {
            "id": str(issue.id),
            "identifier": f"{project.identifier}-{issue.sequence_id}",
            "title": issue.name or "",
            "description": _issue_description_markdown(issue),
            "state": state.name if state else "",
            "state_group": state.group if state else "",
            "priority": issue.priority or "none",
            "labels": labels,
            "assignees": assignees,
            "url": _absolute_issue_url(issue),
            "target_date": issue.target_date.isoformat() if issue.target_date else None,
            "project_states": project_states,
        },
        "workspace": {
            "slug": workspace.slug,
            "name": workspace.name,
        },
        "project": {
            "id": str(project.id),
            "identifier": project.identifier,
            "name": project.name,
        },
        "repo": {
            "url": (getattr(project, "repo_url", "") or None),
            "base_branch": (getattr(project, "base_branch", "") or None),
            "work_branch": (getattr(issue, "git_work_branch", "") or None),
        },
        "parent": (
            {
                "identifier": f"{project.identifier}-{parent.sequence_id}",
                "title": parent.name or "",
                "work_branch": (getattr(parent, "git_work_branch", "") or None),
            }
            if parent is not None
            else None
        ),
        "run": {
            "id": str(run.id),
            "attempt": attempt,
            "turn_number": 1,
        },
    }


def _compute_attempt(issue: Issue, run: AgentRun) -> int:
    """Attempt number = count of prior runs on this issue, plus one.

    Counts every prior run regardless of terminal status — surfacing cancelled
    or failed attempts in the attempt counter is useful context for the agent.
    Cheap, deterministic, and good enough for MVP.
    """
    if issue is None:
        return 1
    prior = AgentRun.objects.filter(work_item_id=issue.id).exclude(id=run.id).count()
    return prior + 1
