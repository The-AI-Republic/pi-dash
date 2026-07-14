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

# Sentinel default for update_issue's parent_issue_id: lets us tell "argument
# omitted" (leave the parent untouched) apart from an explicit ``null`` (unlink),
# since both would otherwise arrive as Python ``None``.
_PARENT_UNSET = "__unset__"


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
        "parent_id": str(issue.parent_id) if issue.parent_id else None,
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


def _resolve_parent(deps, project_id, parent_issue_id, child_id=None) -> Issue:
    """Resolve + validate a parent issue for a child living in ``project_id``.

    Enforces the same constraints the native sub-issue UI does: the parent must
    be accessible to the user, live in the same project as the child, and the
    link must not introduce a cycle (self-parenting or making the child one of
    its own descendants). Returns the parent Issue, or raises ModelRetry.
    """
    parent = _scoping.get_issue(deps, parent_issue_id)  # scope check + ToolNotFound
    if str(parent.project_id) != str(project_id):
        raise ModelRetry("The parent issue must be in the same project as the child issue.")
    if child_id is not None:
        if str(parent.id) == str(child_id):
            raise ModelRetry("An issue can't be its own parent.")
        # Walk the proposed parent's ancestor chain; if the child appears in it,
        # linking would create a cycle. ``seen`` guards against traversing a
        # pre-existing cycle forever.
        ancestor_parent_id = parent.parent_id
        seen: set = set()
        while ancestor_parent_id is not None:
            if str(ancestor_parent_id) == str(child_id):
                raise ModelRetry("That parent link would create a cycle.")
            if ancestor_parent_id in seen:
                break
            seen.add(ancestor_parent_id)
            ancestor_parent_id = (
                Issue.objects.filter(pk=ancestor_parent_id)
                .values_list("parent_id", flat=True)
                .first()
            )
    return parent


@assistant.tool
def create_issue(
    ctx: RunContext[AssistantDeps],
    project_id: str,
    name: str,
    description_md: Optional[str] = None,
    state_id: Optional[str] = None,
    priority: Optional[str] = None,
    parent_issue_id: Optional[str] = None,
) -> dict:
    """Create an issue. Requires Member or Admin role. Pass parent_issue_id to
    link it as a sub-issue of another issue in the same project. (Assignees/labels:
    set them afterwards in the UI — not yet supported by this tool.)"""
    deps = ctx.deps
    project = _scoping.get_project(deps, project_id)
    _scoping.require_project_write(deps, project_id)

    if not name or not name.strip():
        raise ModelRetry("An issue name is required.")
    prio = (priority or "none").lower()
    if prio not in _VALID_PRIORITIES:
        raise ModelRetry(f"Priority must be one of {sorted(_VALID_PRIORITIES)}.")
    state = _resolve_state(deps, project_id, state_id)
    parent = _resolve_parent(deps, project_id, parent_issue_id) if parent_issue_id else None
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
            parent=parent,
            created_by=user,
            created_via=deps.created_via,
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
    parent_issue_id: Optional[str] = _PARENT_UNSET,
) -> dict:
    """Update an issue's name, description, state, or priority. Requires write access.
    Changing the state may dispatch a coding run if it moves the issue into a
    delegated/ticking state (same as moving it in the UI). Pass parent_issue_id to
    re-parent the issue (must be another issue in the same project); pass null to
    unlink it from its current parent; omit it to leave the parent unchanged."""
    deps = ctx.deps
    issue = _scoping.get_issue(deps, issue_id)
    _scoping.require_project_write(deps, str(issue.project_id))

    # Validate inputs before taking any lock.
    prio = None
    if priority is not None:
        prio = priority.lower()
        if prio not in _VALID_PRIORITIES:
            raise ModelRetry(f"Priority must be one of {sorted(_VALID_PRIORITIES)}.")
    to_state = None
    if state_id is not None:
        to_state = _resolve_state(deps, str(issue.project_id), state_id)
    # parent_issue_id: sentinel = untouched, None/"" = unlink, else resolve+validate.
    parent_touched = parent_issue_id != _PARENT_UNSET
    new_parent = None
    if parent_touched and parent_issue_id:
        new_parent = _resolve_parent(deps, str(issue.project_id), parent_issue_id, child_id=issue.id)
    if (
        name is None
        and description_md is None
        and prio is None
        and to_state is None
        and not parent_touched
    ):
        raise ModelRetry("Nothing to update — provide at least one field to change.")

    user = _scoping.user_for(deps)
    run_id = None
    changed: list[str] = []
    # Re-fetch under a row lock so concurrent UI edits aren't lost; only the
    # changed columns are written back via update_fields.
    with impersonate(user), transaction.atomic():
        locked = Issue.objects.select_for_update().get(pk=issue.id)
        from_state = locked.state
        update_fields: list[str] = []
        if name is not None and name.strip():
            locked.name = name.strip()[:255]
            changed.append("name")
            update_fields.append("name")
        if description_md is not None:
            locked.description_html = to_safe_html(description_md)
            locked.description_json = {}
            changed.append("description")
            update_fields += ["description_html", "description_json"]
        if prio is not None:
            locked.priority = prio
            changed.append("priority")
            update_fields.append("priority")
        if to_state is not None:
            locked.state = to_state
            changed.append("state")
            update_fields.append("state")
        if parent_touched:
            locked.parent = new_parent
            changed.append("parent")
            update_fields.append("parent")
        # include audit columns so update_fields doesn't drop them (BaseModel.save
        # sets updated_by from the impersonated user; updated_at is auto_now).
        update_fields += ["updated_at", "updated_by"]
        locked.save(update_fields=update_fields)
        issue = locked

        # Mirror the UI: a state change routes through the orchestration transition
        # handler, which may dispatch a coding run.
        if to_state is not None and from_state != to_state:
            from pi_dash.orchestration import service as orchestration

            outcome = orchestration.handle_issue_state_transition(
                locked, from_state, to_state, actor=user, dispatch_immediate=True
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
