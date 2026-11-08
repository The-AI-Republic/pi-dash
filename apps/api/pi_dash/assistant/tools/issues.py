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
from pi_dash.utils.issue_filters import IssueFilterError, work_item_list_filters

_VALID_PRIORITIES = {"urgent", "high", "medium", "low", "none"}
_SEARCH_LIMIT = 20
_NAME_CAP = 200
_DESC_CAP = 2000
_COMMENT_CAP = 500

# Sentinel default for update_issue's parent_issue_id: lets us tell "argument
# omitted" (leave the parent untouched) apart from an explicit ``null`` (unlink),
# since both would otherwise arrive as Python ``None``.
_PARENT_UNSET = "__unset__"

# Sentinel default for update_issue's assigned_runner / assigned_pod: same
# "omitted vs explicit null" distinction (null clears the assignment).
_EXECUTION_TARGET_UNSET = "__unset__"


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
        "assigned_pod_id": str(issue.assigned_pod_id) if issue.assigned_pod_id else None,
        "assigned_runner_id": str(issue.assigned_runner_id) if issue.assigned_runner_id else None,
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
def list_issues(
    ctx: RunContext[AssistantDeps],
    project_id: str,
    state: Optional[str] = None,
    state_group: Optional[str] = None,
    parent_issue_id: Optional[str] = None,
    labels: Optional[str] = None,
    priority: Optional[str] = None,
    limit: int = _SEARCH_LIMIT,
    offset: int = 0,
) -> dict:
    """List issues in one project, newest activity first, up to 20 per page.
    Optional filters, each comma-separated (values OR together, filters AND
    together): state (names or ids), state_group (backlog, unstarted, started,
    review, test, completed, cancelled), parent_issue_id (issue id or PROJ-123
    identifier, or "null" for top-level issues only), labels (names or ids),
    priority (urgent, high, medium, low, none)."""
    deps = ctx.deps
    _scoping.get_project(deps, project_id)  # scope check
    params = {
        "state": state,
        "state_group": state_group,
        "parent": parent_issue_id,
        "labels": labels,
        "priority": priority,
    }
    try:
        filters = work_item_list_filters(params, project_id=project_id, workspace_slug=deps.workspace_slug)
    except IssueFilterError as e:
        raise ModelRetry(str(e)) from e
    qs = (
        _scoping.scoped_issues(deps)
        .filter(project_id=project_id)
        .filter(**filters)
        .select_related("project", "state")
        .distinct()
        .order_by("-updated_at")
    )
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
    # Execution target (design §14.6): the assignment, its detail, and the
    # waiting state of the latest run (null when nothing is waiting).
    from pi_dash.runner.models import AgentRun
    from pi_dash.runner.services.issue_assignment import assigned_runner_detail, build_dispatch_wait

    latest_run = AgentRun.objects.filter(work_item=issue).order_by("-created_at").first()
    data["assigned_runner_detail"] = assigned_runner_detail(getattr(issue, "assigned_runner", None))
    data["dispatch_wait"] = build_dispatch_wait(latest_run, viewer=_scoping.user_for(ctx.deps))
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


def _is_uuid(value: str) -> bool:
    import uuid

    try:
        uuid.UUID(value)
        return True
    except (ValueError, AttributeError, TypeError):
        return False


def _resolve_runner_ref(deps, project, user, ref):
    """Resolve an ``assigned_runner`` input (UUID or name) in ``project``.

    Names resolve among runners the acting user can see in the project
    (private runners are owner-only); an ambiguous name lists the matches.
    Unknown UUIDs fail closed at the lookup; eligibility itself is owned
    by the shared service (E1-E8), whose codes pass through.
    """
    from pi_dash.runner.models import Runner
    from pi_dash.runner.services.permissions import runner_visible_to_user_q

    ref = (ref or "").strip()
    if not ref:
        raise ModelRetry("assigned_runner must not be empty")
    if _is_uuid(ref):
        runner = Runner.objects.filter(pk=ref, pod__project=project).first()
        if runner is None:
            raise ModelRetry("assigned_runner not found in this project")
        return runner
    rows = list(
        Runner.objects.filter(pod__project=project)
        .filter(runner_visible_to_user_q(user))
        .order_by("name")
    )
    matches = [r for r in rows if r.name.strip().lower() == ref.lower()]
    if len(matches) == 1:
        return matches[0]
    if not matches:
        available = ", ".join(r.name for r in rows) or "(none)"
        raise ModelRetry(f"assigned_runner '{ref}' not found; available: {available}")
    listed = ", ".join(f"{r.name} ({r.id})" for r in matches)
    raise ModelRetry(f"assigned_runner '{ref}' matched {len(matches)} runners; disambiguate with the UUID: {listed}")


def _resolve_pod_ref(project, ref):
    """Resolve an ``assigned_pod`` input (UUID or name) in ``project``.

    Accepts the full ``<PROJ>_<name>`` or the bare ``<name>``.
    """
    from pi_dash.runner.models import Pod

    ref = (ref or "").strip()
    if not ref:
        raise ModelRetry("assigned_pod must not be empty")
    if _is_uuid(ref):
        pod = Pod.objects.filter(pk=ref, project=project).first()
        if pod is None:
            raise ModelRetry("assigned_pod not found in this project")
        return pod
    rows = list(Pod.objects.filter(project=project).order_by("name"))
    matches = [p for p in rows if p.name == ref]
    if not matches:
        matches = [p for p in rows if p.name.split("_", 1)[-1] == ref]
    if len(matches) == 1:
        return matches[0]
    if not matches:
        available = ", ".join(p.name for p in rows) or "(none)"
        raise ModelRetry(f"assigned_pod '{ref}' not found; available: {available}")
    listed = ", ".join(f"{p.name} ({p.id})" for p in matches)
    raise ModelRetry(f"assigned_pod '{ref}' matched {len(matches)} pods; disambiguate with the UUID: {listed}")


def _validate_execution_target_or_retry(attrs, *, issue, project_id, user):
    """Run the shared E-code validation, mapping the API's error code and
    message through to the tool channel unchanged (design §14.6)."""
    from rest_framework import serializers

    from pi_dash.runner.services.issue_assignment import validate_execution_target

    try:
        validate_execution_target(attrs, issue=issue, project_id=project_id, user=user)
    except serializers.ValidationError as exc:
        detail = exc.detail if isinstance(exc.detail, dict) else {"error": exc.detail}
        parts = []
        for details in detail.values():
            if not isinstance(details, list):
                details = [details]
            for d in details:
                code = getattr(d, "code", "")
                parts.append(f"{d} [{code}]" if code else str(d))
        raise ModelRetry("; ".join(parts)) from exc


@assistant.tool
def create_issue(
    ctx: RunContext[AssistantDeps],
    project_id: str,
    name: str,
    description_md: Optional[str] = None,
    state_id: Optional[str] = None,
    priority: Optional[str] = None,
    parent_issue_id: Optional[str] = None,
    assigned_runner: Optional[str] = None,
    assigned_pod: Optional[str] = None,
) -> dict:
    """Create an issue. Requires Member or Admin role. Pass parent_issue_id to
    link it as a sub-issue of another issue in the same project. Pass
    assigned_runner (UUID or name) to dedicate the issue to one runner and
    assigned_pod (UUID or name) to set its pod; eligibility follows the
    shared E-code rules and any E-code is returned to the model. (Assignees/labels:
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
    target_pod = _resolve_pod_ref(project, assigned_pod) if assigned_pod else None
    target_runner = (
        _resolve_runner_ref(deps, project, user, assigned_runner) if assigned_runner else None
    )
    if target_runner is not None:
        _validate_execution_target_or_retry(
            {
                "assigned_runner": target_runner,
                "assigned_pod": target_pod or target_runner.pod,
            },
            issue=None,
            project_id=project_id,
            user=user,
        )

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
            assigned_pod=target_pod or (target_runner.pod if target_runner else None),
            assigned_runner=target_runner,
        )
        if target_runner is not None:
            from django.utils import timezone as dj_timezone

            from pi_dash.runner.services.issue_assignment import apply_execution_target

            apply_execution_target(
                issue,
                new_runner=target_runner,
                actor=user,
                decided_by=dj_timezone.now(),
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
    assigned_runner: Optional[str] = _EXECUTION_TARGET_UNSET,
    assigned_pod: Optional[str] = _EXECUTION_TARGET_UNSET,
) -> dict:
    """Update an issue's name, description, state, priority, parent, pod or
    dedicated runner. Requires write access. Changing the state may dispatch
    a coding run if it moves the issue into a delegated/ticking state (same
    as moving it in the UI). Pass parent_issue_id to re-parent the issue
    (must be another issue in the same project); pass null to unlink it
    from its current parent; omit it to leave the parent unchanged.
    assigned_runner (UUID or name) dedicates the issue to one runner; null
    clears it back to "any runner in the pod". assigned_pod (UUID or name)
    sets the pod; null clears it; a pod change with no runner in the same
    update also clears the dedicated runner (§13.1). Eligibility follows
    the shared E-code rules and any E-code is returned to the model."""
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
    # Execution target (dedicated runner / pod): sentinel = untouched,
    # None = clear, else resolve the ref. Resolution happens before the
    # lock; E-code validation happens on the locked row below.
    user = _scoping.user_for(deps)
    runner_touched = assigned_runner != _EXECUTION_TARGET_UNSET
    pod_touched = assigned_pod != _EXECUTION_TARGET_UNSET
    project = issue.project
    new_runner = (
        _resolve_runner_ref(deps, project, user, assigned_runner)
        if runner_touched and assigned_runner
        else None
    )
    new_pod = _resolve_pod_ref(project, assigned_pod) if pod_touched and assigned_pod else None
    if (
        name is None
        and description_md is None
        and prio is None
        and to_state is None
        and not parent_touched
        and not runner_touched
        and not pod_touched
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
        # The save fires the orchestration signal; let it update the clock
        # only and dispatch below with ``actor`` so one move makes one run
        # (the signal would otherwise dispatch first and the explicit call
        # would queue a second, free entry behind it).
        from pi_dash.orchestration.signals import _DISPATCH_IMMEDIATE_ATTR

        setattr(locked, _DISPATCH_IMMEDIATE_ATTR, False)
        locked.save(update_fields=update_fields)
        issue = locked

        # Execution target (dedicated runner / pod), validated on the
        # locked row through the shared service; pod follows the runner
        # (§7.2). A pod-only change clears a dedicated runner (§13.1).
        if runner_touched or pod_touched:
            if pod_touched:
                new_pod_id = new_pod.id if new_pod is not None else None
                if str(new_pod_id) != str(locked.assigned_pod_id) and locked.has_active_run:
                    raise ModelRetry("cannot reassign pod while the issue has an active run")
            attrs: dict = {}
            if runner_touched:
                attrs["assigned_runner"] = new_runner
            if pod_touched:
                attrs["assigned_pod"] = new_pod
            _validate_execution_target_or_retry(
                attrs,
                issue=locked,
                project_id=str(locked.project_id),
                user=user,
            )
            from django.utils import timezone as dj_timezone

            from pi_dash.runner.services.issue_assignment import (
                apply_execution_target,
                clear_dedicated_runner,
            )

            if pod_touched:
                locked.assigned_pod = new_pod
                locked.save(update_fields=["assigned_pod", "updated_at"])
                changed.append("pod")
            if runner_touched:
                apply_execution_target(
                    locked,
                    new_runner=new_runner,
                    actor=user,
                    decided_by=dj_timezone.now(),
                )
                changed.append("runner")
            elif pod_touched and locked.assigned_runner_id is not None:
                clear_dedicated_runner(
                    locked,
                    reason="pod changed",
                    actor=user,
                    decided_by=None,
                )

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


# --- Relations (PDASHOSS01-199) ---------------------------------------------
# Same vocabulary and output shape as ``pidash issue relate|unrelate|relations``
# and the MCP connector; the logic lives in ``pi_dash.orchestration.relations``.
# Issue references may be identifiers (PROJ-12) or UUIDs, resolved only within
# the user's member projects, so a relation can never reach an issue the user
# cannot see.


def _resolve_issue_refs(deps, refs: list[str]) -> list[Issue]:
    from pi_dash.orchestration import relations

    found, unresolved = relations.resolve_refs(refs, _scoping.scoped_issues(deps))
    if unresolved:
        raise _scoping.ToolNotFound("Issues not found or not accessible: " + ", ".join(unresolved))
    return found


def _relations_view(deps, issue) -> dict:
    from pi_dash.orchestration import relations

    grouped = relations.grouped_relations(issue, _scoping.scoped_issues(deps))
    for items in grouped.values():
        for item in items:
            name, _ = _results.truncate(item["name"], _NAME_CAP)
            item["name"] = _results.wrap_untrusted(name)
    return grouped


def _relation_write(ctx, issue: str, relation_type: str, related_issues: list[str], operation, verb: str) -> dict:
    from pi_dash.orchestration import relations

    deps = ctx.deps
    (source,) = _resolve_issue_refs(deps, [issue])
    _scoping.require_project_write(deps, str(source.project_id))
    if not related_issues:
        raise ModelRetry("related_issues must list at least one issue.")
    targets = _resolve_issue_refs(deps, related_issues)
    user = _scoping.user_for(deps)
    try:
        with impersonate(user):
            result = operation(source, relation_type, targets, user)
    except relations.RelationError as exc:
        raise ModelRetry(str(exc)) from exc
    changed = result.get("created") or result.get("removed") or []
    if changed:
        _results.record_write(
            deps,
            f"{verb} {_identifier(source)} {result['relation_type']} {', '.join(changed)}",
            links=[_results.issue_link(deps, source)],
        )
    result["relations"] = _relations_view(deps, source)
    return result


@assistant.tool
def relate_issues(
    ctx: RunContext[AssistantDeps],
    issue: str,
    relation_type: str,
    related_issues: list[str],
) -> dict:
    """Record a relation from ``issue`` to each of ``related_issues`` (identifiers
    like PROJ-12, or UUIDs). relation_type is read from ``issue``'s side: one of
    blocked_by, blocking, relates_to, duplicate, start_before, start_after,
    finish_before, finish_after, implemented_by, implements. Use blocked_by when
    ``issue`` cannot finish until the related issues do. Idempotent: pairs that
    already have this relation come back under ``unchanged``; pairs that already
    have a different relation come back under ``conflicts`` and are not changed.
    Requires write access to ``issue``'s project."""
    from pi_dash.orchestration import relations

    return _relation_write(ctx, issue, relation_type, related_issues, relations.relate, "Related")


@assistant.tool
def unrelate_issues(
    ctx: RunContext[AssistantDeps],
    issue: str,
    relation_type: str,
    related_issues: list[str],
) -> dict:
    """Remove the ``relation_type`` relation between ``issue`` and each of
    ``related_issues``. Only that exact relation is removed; pairs without it come
    back under ``not_related`` (not an error). Requires write access."""
    from pi_dash.orchestration import relations

    return _relation_write(ctx, issue, relation_type, related_issues, relations.unrelate, "Unrelated")


@assistant.tool
def list_issue_relations(ctx: RunContext[AssistantDeps], issue: str) -> dict:
    """List every relation of an issue (identifier or UUID), grouped by type from
    its side (blocked_by, blocking, relates_to, ...), each with identifier, name
    and state. Check ``blocked_by`` states before starting dependent work."""
    deps = ctx.deps
    (source,) = _resolve_issue_refs(deps, [issue])
    return {"issue": _identifier(source), "relations": _relations_view(deps, source)}
