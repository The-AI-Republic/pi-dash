# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Issue query + write tools."""

from __future__ import annotations

from typing import Optional

from crum import impersonate
from django.db import transaction
from django.db.models import Max
from pydantic_ai import ModelRetry, RunContext

from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.runtime.markdown import to_safe_html
from pi_dash.assistant.tools import _results, _scoping
from pi_dash.db.models import Issue
from pi_dash.search.issue import issue_search_queryset

_VALID_PRIORITIES = {"urgent", "high", "medium", "low", "none"}
_SEARCH_LIMIT = 20
_NAME_CAP = 200
_DESC_CAP = 2000
_COMMENT_CAP = 500


def _identifier(issue) -> str:
    ident = getattr(issue.project, "identifier", "") if issue.project_id else ""
    return f"{ident}-{issue.sequence_id}" if ident else str(issue.sequence_id)


def _brief(issue) -> dict:
    name, name_trunc = _results.truncate(issue.name, _NAME_CAP)
    return {
        "id": str(issue.id),
        "identifier": _identifier(issue),
        "project_id": str(issue.project_id),
        "name": _results.wrap_untrusted(name),
        "name_truncated": name_trunc,
        "state": issue.state.name if issue.state_id else None,
        "state_group": issue.state.group if issue.state_id else None,
        "priority": issue.priority,
    }


@assistant.tool
def search_issues(
    ctx: RunContext[AssistantDeps],
    query: str,
    project_id: Optional[str] = None,
    limit: int = _SEARCH_LIMIT,
    offset: int = 0,
) -> dict:
    """Full-text search issues you can access. Returns up to 20 results per page."""
    qs = _scoping.scoped_issues(ctx.deps).select_related("project", "state")
    if project_id:
        _scoping.get_project(ctx.deps, project_id)  # scope check
        qs = qs.filter(project_id=project_id)
    if query.strip():
        qs = issue_search_queryset(qs, query).distinct()
    qs = qs.order_by("-updated_at")
    limit = max(1, min(int(limit or _SEARCH_LIMIT), _SEARCH_LIMIT))
    offset = max(0, int(offset or 0))
    window = list(qs[offset : offset + limit + 1])
    has_more = len(window) > limit
    return {
        "results": [_brief(i) for i in window[:limit]],
        "has_more": has_more,
        "next_offset": offset + limit if has_more else None,
    }


@assistant.tool
def list_my_issues(
    ctx: RunContext[AssistantDeps],
    scope: str = "all",
    limit: int = _SEARCH_LIMIT,
    offset: int = 0,
) -> dict:
    """List issues you're involved in. scope: 'all', 'assigned', or 'created'."""
    if scope not in {"all", "assigned", "created"}:
        scope = "all"
    qs = _scoping.my_issues(ctx.deps, scope).select_related("project", "state").order_by("-updated_at")
    limit = max(1, min(int(limit or _SEARCH_LIMIT), _SEARCH_LIMIT))
    offset = max(0, int(offset or 0))
    window = list(qs[offset : offset + limit + 1])
    has_more = len(window) > limit
    return {
        "results": [_brief(i) for i in window[:limit]],
        "has_more": has_more,
        "next_offset": offset + limit if has_more else None,
    }


@assistant.tool
def get_issue(ctx: RunContext[AssistantDeps], issue_id: str) -> dict:
    """Get one issue in detail, including its most recent comments."""
    issue = _scoping.get_issue(ctx.deps, issue_id)
    desc, desc_trunc = _results.truncate(issue.description_stripped or "", _DESC_CAP)
    comments = []
    for c in issue.issue_comments.select_related("actor").order_by("-created_at")[:10]:
        body, body_trunc = _results.truncate(c.comment_stripped or "", _COMMENT_CAP)
        author = "Pi Dash AI" if c.speaker_type == "agent" else (
            (c.actor.display_name or c.actor.email) if c.actor_id else "Unknown"
        )
        comments.append(
            {
                "author": author,
                "body": _results.wrap_untrusted(body),
                "body_truncated": body_trunc,
                "created_at": c.created_at.isoformat(),
            }
        )
    data = _brief(issue)
    data.update(
        {
            "description": _results.wrap_untrusted(desc),
            "description_truncated": desc_trunc,
            "recent_comments": list(reversed(comments)),
        }
    )
    return data


def _resolve_state(deps, project_id, state_id):
    states = _scoping.project_states(deps, project_id)
    if state_id:
        st = states.filter(id=state_id).first()
        if st is None:
            raise ModelRetry(f"State {state_id} is not a valid state for this project.")
        return st
    return states.filter(default=True).first() or states.order_by("sequence").first()


@assistant.tool
def create_issue(
    ctx: RunContext[AssistantDeps],
    project_id: str,
    name: str,
    description_md: Optional[str] = None,
    state_id: Optional[str] = None,
    priority: Optional[str] = None,
) -> dict:
    """Create an issue. Requires Member or Admin role. (Assignees/labels: set them
    afterwards in the UI — not yet supported by this tool.)"""
    deps = ctx.deps
    project = _scoping.get_project(deps, project_id)
    _scoping.require_project_write(deps, project_id)

    if not name or not name.strip():
        raise ModelRetry("An issue name is required.")
    prio = (priority or "none").lower()
    if prio not in _VALID_PRIORITIES:
        raise ModelRetry(f"Priority must be one of {sorted(_VALID_PRIORITIES)}.")
    state = _resolve_state(deps, project_id, state_id)
    user = _scoping.user_for(deps)

    # impersonate so BaseModel.save() attributes created_by to the acting user
    # (the tool runs in Celery with no request, so crum's current user is None).
    with impersonate(user), transaction.atomic():
        # Per-project sequence allocation under a project-row lock.
        from pi_dash.db.models import Project

        Project.objects.select_for_update().get(pk=project_id)
        next_seq = (
            Issue.objects.filter(project_id=project_id).aggregate(Max("sequence_id"))["sequence_id__max"]
            or 0
        ) + 1
        issue = Issue.objects.create(
            name=name.strip()[:255],
            description_html=to_safe_html(description_md),
            description_json={},
            priority=prio,
            sequence_id=next_seq,
            state=state,
            project=project,
            workspace=project.workspace,
            created_by=user,
            created_via="assistant",
        )

    _results.record_write(
        deps,
        f"Created issue {_identifier(issue)} — {issue.name}",
        links=[_results.issue_link(deps, issue)],
    )
    return {"created": True, **_brief(issue), "url_path": _results.issue_link(deps, issue)["url_path"]}


@assistant.tool
def update_issue(
    ctx: RunContext[AssistantDeps],
    issue_id: str,
    name: Optional[str] = None,
    description_md: Optional[str] = None,
    state_id: Optional[str] = None,
    priority: Optional[str] = None,
) -> dict:
    """Update an issue's name, description, state, or priority. Requires write access.
    Changing the state may dispatch a coding run if it moves the issue into a
    delegated/ticking state (same as moving it in the UI)."""
    deps = ctx.deps
    issue = _scoping.get_issue(deps, issue_id)
    _scoping.require_project_write(deps, str(issue.project_id))

    from_state = issue.state
    changed = []
    to_state = None
    if name is not None and name.strip():
        issue.name = name.strip()[:255]
        changed.append("name")
    if description_md is not None:
        issue.description_html = to_safe_html(description_md)
        issue.description_json = {}
        changed.append("description")
    if priority is not None:
        prio = priority.lower()
        if prio not in _VALID_PRIORITIES:
            raise ModelRetry(f"Priority must be one of {sorted(_VALID_PRIORITIES)}.")
        issue.priority = prio
        changed.append("priority")
    if state_id is not None:
        to_state = _resolve_state(deps, str(issue.project_id), state_id)
        issue.state = to_state
        changed.append("state")

    if not changed:
        raise ModelRetry("Nothing to update — provide at least one field to change.")

    user = _scoping.user_for(deps)
    run_id = None
    with impersonate(user):
        issue.save()

        # Mirror the UI: a state change routes through the orchestration transition
        # handler, which may dispatch a coding run.
        if to_state is not None and from_state != to_state:
            from pi_dash.orchestration import service as orchestration

            outcome = orchestration.handle_issue_state_transition(
                issue, from_state, to_state, actor=user, dispatch_immediate=True
            )
            if outcome.created_run is not None:
                run_id = str(outcome.created_run.id)

    _results.record_write(
        deps,
        f"Updated issue {_identifier(issue)} ({', '.join(changed)})",
        links=[_results.issue_link(deps, issue)],
    )
    result = {"updated": True, "changed": changed, **_brief(issue)}
    if run_id:
        result["dispatched_run_id"] = run_id
    return result
