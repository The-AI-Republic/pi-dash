# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Template context builder.

The *only* data a Jinja template ever sees. Ordinary ORM objects must not leak
into `renderer.render()` — the sandboxed environment has very little ability to
defend against unintended attribute access on them.
"""

from __future__ import annotations

import json
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
    return f"/{ws}/projects/{issue.project_id}/issues/{issue.id}" if ws else ""


def _actor_label(actor) -> str:
    if actor is None:
        return "Unknown"
    return (
        getattr(actor, "display_name", None)
        or getattr(actor, "email", None)
        or getattr(actor, "username", None)
        or "Unknown user"
    )


def _comment_author_label(comment) -> str:
    """Render an audience-friendly speaker label for a comment.

    Bot comments are flattened to a single ``Pi Dash Agent`` label so the
    agent reading its own prior posts immediately recognizes them as
    self-authored. Explicit speaker metadata wins over the authenticated
    actor because agent CLI comments may be submitted with a human token.
    """
    actor = comment.actor
    speaker_type = getattr(comment, "speaker_type", None) or "human"
    speaker_label = (getattr(comment, "speaker_label", None) or "").strip()
    actor_label = _actor_label(actor)

    if speaker_type == "agent":
        label = speaker_label or "AI Agent"
        if actor is not None and not getattr(actor, "is_bot", False):
            return f"AI agent: {label} (submitted by {actor_label})"
        return f"AI agent: {label}"
    if speaker_type == "system":
        return f"System: {speaker_label or 'Pi Dash'}"
    if speaker_type == "integration":
        return f"Integration: {speaker_label or actor_label}"
    if actor is None:
        return "Unknown"
    if getattr(actor, "is_bot", False):
        return "AI agent: Pi Dash Agent"
    return f"Human: {actor_label}"


def _comments_section(issue: Issue) -> str:
    """Render the issue's full comment thread as a numbered chronological log.

    Includes both human-authored and agent-authored (bot) comments —
    a continuation run needs to see its own prior question alongside the
    human's reply so it can pick up the conversation. Each entry is
    formatted as ``### Comment N — <author> at <ISO timestamp>`` followed
    by the comment body, separated by blank lines.
    """
    from pi_dash.db.models.issue import IssueComment

    comments = (
        IssueComment.objects.filter(issue=issue)
        .select_related("actor")
        .order_by("created_at")
    )
    parts: list[str] = []
    index = 0
    for comment in comments:
        body = (comment.comment_stripped or "").strip()
        if not body:
            continue
        index += 1
        author = _comment_author_label(comment)
        timestamp = (
            comment.created_at.isoformat() if comment.created_at else "unknown time"
        )
        run_id = getattr(comment, "speaker_agent_run_id", None)
        run_line = f"\nAgent run: {run_id}" if run_id else ""
        parts.append(
            f"### Comment {index} — {author} at {timestamp}{run_line}\n\n{body}"
        )
    if not parts:
        return "(no comments on this issue yet)"
    return "\n\n".join(parts)


def _parent_done_payload(issue: Issue, run: AgentRun) -> str:
    """Return the implementation run payload the review prompt should inspect.

    Review entry intentionally creates a fresh run with ``parent_run=None``.
    The implementation parent is therefore stored on the issue ticker during
    the In Progress -> In Review transition. Fall back to ``run.parent_run``
    for tests and any future non-fresh review entry path.
    """
    parent_run = getattr(run, "parent_run", None)
    if parent_run is None:
        ticker = getattr(issue, "agent_ticker", None)
        parent_run = getattr(ticker, "resume_parent_run", None)
    payload = getattr(parent_run, "done_payload", None) if parent_run is not None else None
    if not payload:
        return "(no parent run done payload available)"
    return json.dumps(payload, indent=2, sort_keys=True)


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
            "description": project.description or "",
        },
        "repo": {
            "url": (getattr(project, "repo_url", "") or None),
            "base_branch": (getattr(project, "base_branch", "") or None),
            "work_branch": (getattr(issue, "git_work_branch", "") or None),
        },
        "parent": (
            {
                "identifier": f"{parent.project.identifier}-{parent.sequence_id}",
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
        "comments_section": _comments_section(issue),
        "parent_done_payload": _parent_done_payload(issue, run),
        # Prior-run workpad body (empty on first run). Surfaced up front so
        # continuation runs see their predecessor's plan/phase/notes without
        # an extra ``pidash workpad get`` round-trip.
        "workpad_body": issue.workpad or "",
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
