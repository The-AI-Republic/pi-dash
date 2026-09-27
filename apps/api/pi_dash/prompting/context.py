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
from typing import Any, Dict, Optional

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


def _issue_identifier(issue: Issue) -> str:
    """Workspace-scoped identifier, e.g. ``TP-12``.

    Always uses the issue's *own* project identifier — a parent may live in a
    different project than its child, so this must not assume the child's
    project.
    """
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _issue_comment_count(issue: Issue) -> int:
    """Count of comments on ``issue`` — matches what ``pidash comment list``
    returns. Surfaced for a parent so the agent learns the discussion volume
    without inlining the parent's comment bodies into this prompt.
    """
    from pi_dash.db.models.issue import IssueComment

    return IssueComment.objects.filter(issue=issue).count()


def _ancestor_chain(issue: Issue) -> list[Issue]:
    """Return ``[issue, parent, grandparent, ... root]``.

    Walks the ``parent`` self-FK upward. The FK has no DB-level acyclicity
    guarantee, so defend against accidental cycles with a visited-id set and a
    hard depth cap — a malformed graph must never spin the renderer.
    """
    chain: list[Issue] = []
    seen: set[Any] = set()
    current: Issue | None = issue
    while current is not None and current.id not in seen and len(chain) < 50:
        chain.append(current)
        seen.add(current.id)
        current = current.parent
    return chain


#: Upper bound on how many children / related work items are inlined into the
#: "Work item relationships" section. An issue with a huge fan-out of sub-items
#: or cross-links must not blow up the prompt; beyond this cap the extra items
#: are simply not listed (the required-reading directive still points the agent
#: at the CLI for anything it needs to chase further).
_MAX_RELATIONSHIP_ITEMS = 25


def _issue_ref(issue: Issue) -> Dict[str, Any]:
    """Compact ``{identifier, title, state}`` for a connected work item.

    Used for children and related ("relates_to") work items in the relationships
    section — enough to recognize and fetch an item, without inlining its body.
    """
    state = getattr(issue, "state", None)
    return {
        "identifier": _issue_identifier(issue),
        "title": issue.name or "",
        "state": state.name if state else "",
    }


def _children_context(issue: Issue) -> list[Dict[str, Any]]:
    """Direct child issues (one level down) as ``{identifier, title, state}``.

    Uses ``issue_objects`` so triage / draft / archived children are excluded —
    the same manager the rest of the app counts sub-issues through. Ordered
    oldest-first (creation order mirrors how scope was broken out) and capped at
    ``_MAX_RELATIONSHIP_ITEMS`` so a large fan-out can't blow up the prompt.
    """
    children = (
        Issue.issue_objects.filter(parent=issue)
        .select_related("state", "project")
        .order_by("created_at")
    )
    return [_issue_ref(child) for child in children[:_MAX_RELATIONSHIP_ITEMS]]


def _related_context(issue: Issue) -> list[Dict[str, Any]]:
    """``relates_to`` work items, both link directions merged and deduped.

    ``relates_to`` is symmetric (``IssueRelationChoices._RELATION_PAIRS`` marks
    it as its own inverse), so a link created from either side must surface here.
    Query both endpoints — the same ``Q(issue_id=...) | Q(related_issue_id=...)``
    shape as ``app/views/issue/relation.py`` — collect the *other* end of each
    relation, dedupe (the two directions can both exist as rows), skip self /
    soft-deleted-target rows, and cap at ``_MAX_RELATIONSHIP_ITEMS``. The default
    manager already excludes soft-deleted relations.
    """
    from django.db.models import Q

    from pi_dash.db.models.issue import IssueRelation

    relations = (
        IssueRelation.objects.filter(relation_type="relates_to")
        .filter(Q(issue_id=issue.id) | Q(related_issue_id=issue.id))
        .select_related(
            "issue__state",
            "issue__project",
            "related_issue__state",
            "related_issue__project",
        )
        .order_by("-created_at")
    )
    seen: set[Any] = set()
    out: list[Dict[str, Any]] = []
    for rel in relations:
        other = rel.related_issue if rel.issue_id == issue.id else rel.issue
        if other is None or other.id == issue.id or other.id in seen:
            continue
        seen.add(other.id)
        out.append(_issue_ref(other))
        if len(out) >= _MAX_RELATIONSHIP_ITEMS:
            break
    return out


#: Directional relation types surfaced in the relationships section, in render
#: order, with the phrase that reads naturally before the other item
#: ("this item <phrase> X"). ``blocked_by`` / ``blocking`` get their own groups;
#: the rest render together under "Other relations". Keys cover both the stored
#: forward types and their inverses from ``IssueRelationChoices._REVERSE_MAPPING``
#: — only forward types are ever written, so the inverse is what the *other*
#: end of a row sees.
_DIRECTIONAL_RELATION_LABELS: Dict[str, str] = {
    "blocked_by": "Blocked by",
    "blocking": "Blocking",
    "start_before": "Starts before",
    "start_after": "Starts after",
    "finish_before": "Finishes before",
    "finish_after": "Finishes after",
    "implemented_by": "Implemented by",
    "implements": "Implements",
}

#: State groups that mean a blocker no longer holds this item back.
_CLOSED_STATE_GROUPS = frozenset({"completed", "cancelled"})


def _directional_relations_context(issue: Issue) -> Dict[str, list[Dict[str, Any]]]:
    """Directional relations keyed by type *as seen from ``issue``*.

    A row ``(issue=A, related_issue=B, relation_type=T)`` means "A T B", so A
    sees B under ``T`` and B sees A under the reverse of ``T`` (``blocked_by``
    -> ``blocking`` etc.). Same both-endpoints query shape as
    ``_related_context``: collect the other end of each row, dedupe per type,
    skip self and soft-deleted targets, and cap each type at
    ``_MAX_RELATIONSHIP_ITEMS``. Each item is ``{identifier, title, state,
    state_group}`` — ``state_group`` lets the template tell an open blocker
    from a finished one. Every key in ``_DIRECTIONAL_RELATION_LABELS`` is
    present (empty list when none).
    """
    from django.db.models import Q

    from pi_dash.db.models.issue import IssueRelation, IssueRelationChoices

    reverse = IssueRelationChoices._REVERSE_MAPPING
    forward_types = [t for t in reverse if t in _DIRECTIONAL_RELATION_LABELS]
    relations = (
        IssueRelation.objects.filter(relation_type__in=forward_types)
        .filter(Q(issue_id=issue.id) | Q(related_issue_id=issue.id))
        # The default manager drops soft-deleted relation rows; a soft-deleted
        # work item on either end must not surface either.
        .filter(issue__deleted_at__isnull=True, related_issue__deleted_at__isnull=True)
        .select_related(
            "issue__state",
            "issue__project",
            "related_issue__state",
            "related_issue__project",
        )
        .order_by("-created_at")
    )
    out: Dict[str, list[Dict[str, Any]]] = {key: [] for key in _DIRECTIONAL_RELATION_LABELS}
    seen: Dict[str, set[Any]] = {key: set() for key in _DIRECTIONAL_RELATION_LABELS}
    for rel in relations:
        if rel.issue_id == issue.id:
            other, kind = rel.related_issue, rel.relation_type
        else:
            other, kind = rel.issue, reverse[rel.relation_type]
        if other is None or other.id == issue.id:
            continue
        if other.id in seen[kind] or len(out[kind]) >= _MAX_RELATIONSHIP_ITEMS:
            continue
        seen[kind].add(other.id)
        state = getattr(other, "state", None)
        out[kind].append({**_issue_ref(other), "state_group": state.group if state else ""})
    return out


def _relations_context(issue: Issue) -> Dict[str, Any]:
    """Context keys for the directional groups of the relationships section.

    ``blocked_by`` / ``blocking`` are lists of work-item refs; ``other_relations``
    flattens the remaining directional types into one list whose items carry a
    human ``relation`` label ("Starts before", "Implements", ...).
    ``open_blockers`` names the ``blocked_by`` items not yet completed or
    cancelled, and ``has_open_blockers`` is its truthiness — the template warns
    on it and the agent decides whether it can proceed.
    """
    by_type = _directional_relations_context(issue)
    blocked_by = by_type["blocked_by"]
    open_blockers = [b["identifier"] for b in blocked_by if b["state_group"] not in _CLOSED_STATE_GROUPS]
    other_relations = [
        {**item, "relation": label}
        for kind, label in _DIRECTIONAL_RELATION_LABELS.items()
        if kind not in ("blocked_by", "blocking")
        for item in by_type[kind]
    ]
    return {
        "blocked_by": blocked_by,
        "blocking": by_type["blocking"],
        "other_relations": other_relations,
        "open_blockers": open_blockers,
        "has_open_blockers": bool(open_blockers),
    }


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
    """Render the issue's unfolded comments as a numbered chronological log.

    Includes both human-authored and agent-authored (bot) comments, except
    comments explicitly labeled ``fold`` —
    a continuation run needs to see its own prior question alongside the
    human's reply so it can pick up the conversation. Each entry is
    formatted as ``### Comment N — <author> at <ISO timestamp>`` followed
    by the comment body, separated by blank lines.
    """
    from pi_dash.db.models.issue import IssueComment

    comments = (
        IssueComment.objects.filter(issue=issue)
        .exclude(labels__contains=["fold"])
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
        timestamp = comment.created_at.isoformat() if comment.created_at else "unknown time"
        run_id = getattr(comment, "speaker_agent_run_id", None)
        run_line = f"\nAgent run: {run_id}" if run_id else ""
        parts.append(f"### Comment {index} — {author} at {timestamp}{run_line}\n\n{body}")
    if not parts:
        return "(no comments on this issue yet)"
    return "\n\n".join(parts)


def _humanize_interval(seconds: int) -> str:
    """Render an interval for prose ("3 hours", "90 minutes")."""
    if seconds % 3600 == 0:
        hours = seconds // 3600
        return f"{hours} hour" + ("s" if hours != 1 else "")
    minutes = max(1, round(seconds / 60))
    return f"{minutes} minute" + ("s" if minutes != 1 else "")


def _tick_context(issue: Issue) -> Optional[Dict[str, Any]]:
    """Surface the issue's budget pool and clock for the prompt.

    One pool per issue (``.ai_design/ticking_relevance/design.md`` §5.3):
    the agent is told how many machine-started runs the issue has used and
    how many remain **including when the clock is stopped** — that is
    exactly the spent-pool case the budget line exists to warn about.
    ``cap`` / ``remaining`` are ``None`` for an infinite (``-1``) pool so
    templates can branch with ``{% if tick.cap is not none %}``.

    ``count`` / ``cap`` are the raw counters: a wait run is a run
    (PDASHOSS01-211). ``pidash issue wait`` adds one to ``used`` (the run it
    ended) and one to the cap (the tick it bought back), so waiting costs no
    net budget and still shows up. ``waited`` is reported alongside so a
    reader can tell how much of the count went on discovering a blocker.

    Returns ``None`` only when no ticker row exists (the issue has never
    entered the ticking bucket) or when the configured cadence is nonsense
    (the project fields are API-writable with no validation; "every 0
    hours" or "of -2 runs" must not reach a prompt).
    """
    from pi_dash.db.models.issue_agent_ticker import INFINITE_MAX_TICKS

    # Reverse OneToOne — RelatedObjectDoesNotExist subclasses AttributeError,
    # so getattr's default covers issues that never armed a ticker.
    ticker = getattr(issue, "agent_ticker", None)
    if ticker is None:
        return None
    cap = ticker.effective_max_ticks()
    interval = ticker.effective_interval_seconds()
    if interval <= 0:
        return None
    if cap != INFINITE_MAX_TICKS and cap < 0:
        return None
    unlimited = cap == INFINITE_MAX_TICKS
    remaining = None if unlimited else max(0, cap - ticker.used)
    # A wait run is a run (PDASHOSS01-211). Report the counters as they
    # stand: ``cap`` already carries the ticks waits bought back, so an
    # issue that spent one run discovering a blocker reads "1 of 11", not
    # "0 of 10". Netting waits out of ``count``/``cap`` while ``remaining``
    # kept the raw figure made the three numbers disagree, and the clamps
    # hid a real run once ``waited`` ran ahead of ``used``.
    waited = ticker.waited
    return {
        "count": ticker.used,
        "cap": None if unlimited else cap,
        "remaining": remaining,
        # How many times this issue has ended a run by waiting on a blocker,
        # and how many such waits it may still make for free.
        "waited": waited,
        "wait_allowance": ticker.wait_allowance(),
        # ``used`` already counts this run when the ticker started it, so
        # ``remaining == 0`` means "no machine-started run follows this one".
        "spent": (not unlimited) and remaining == 0,
        "clock_live": bool(ticker.enabled),
        "interval_seconds": interval,
        "interval_human": _humanize_interval(interval),
    }


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


def _issue_run_kind(issue: Issue) -> str:
    """Resolve the prompt *kind* for an issue run, for ``run.kind`` in context.

    Shared sections branch on ``run.kind`` (always defined) — e.g. the CLI
    section guards issue-specific lines with ``run.kind != "scheduler"``. Both
    issue kinds (coding-task / review) are non-scheduler, so issue content
    always renders for issue runs.
    """
    from pi_dash.orchestration.agent_phases import template_name_for
    from pi_dash.prompting import recipes

    state = getattr(issue, "state", None)
    return recipes.kind_for(template_name_for(state))


def _repo_context(project, issue: Issue) -> Dict[str, Any]:
    """Return provider-neutral repository prompt context for an issue run."""
    repo = {
        "url": (getattr(project, "repo_url", "") or None),
        "base_branch": (getattr(project, "base_branch", "") or None),
        "work_branch": (getattr(issue, "git_work_branch", "") or None),
        "provider": None,
        "provider_display_name": "Git provider",
        "host_url": None,
        "full_name": None,
        "code_review_term": "code review",
    }
    from pi_dash.db.models import GitRepositoryBinding
    from pi_dash.integrations.git.registry import get_adapter

    binding = GitRepositoryBinding.objects.filter(project=project).select_related("repository").first()
    if binding is None:
        return repo
    remote = binding.repository
    try:
        adapter = get_adapter(remote.provider)
        provider_display_name = adapter.display_name
        code_review_term = adapter.code_review_term
    except KeyError:
        provider_display_name = remote.provider.title()
        code_review_term = "code review"
    repo.update(
        {
            "provider": remote.provider,
            "provider_display_name": provider_display_name,
            "host_url": remote.host_url,
            "full_name": remote.full_name,
            "code_review_term": code_review_term,
        }
    )
    return repo


def _code_reviews_context(issue: Issue) -> list[Dict[str, Any]]:
    """Return the git code reviews (PRs/MRs) attached to ``issue``.

    Surfaces the ``GitCodeReviewLink`` rows created via ``pidash issue
    attach-review`` or provider webhooks so the agent learns an issue already
    has associated PRs before it opens a new one. Provider-neutral — the same
    shape describes a GitHub pull request, a GitLab merge request, etc. The
    reverse relation uses the model's default (soft-delete-filtering) manager,
    so removed links never leak into the prompt. Ordered newest-first, matching
    ``GitCodeReviewLink.Meta.ordering``.
    """
    return [
        {
            "url": cr.url,
            "title": cr.title or "",
            "state": cr.state,
            "merged": bool(cr.merged),
            "draft": bool(cr.draft),
            "provider": cr.provider,
            "external_iid": cr.external_iid,
        }
        for cr in issue.git_code_reviews.all()
    ]


def extra_toolsets_vars(run) -> Dict[str, Any]:
    """Prompt variables for deployment-provided toolsets.

    ``extra_toolsets`` is read from the run's own plan snapshot, the same flag
    ``cloud_agent.runtime`` gates on, so the prompt can never claim tools the
    run will not be given. The prompt has to say they exist at all because
    their names are not knowable at plan time and so never reach
    ``available_tools``.

    The schema-fetch tool is named by the seam rather than here: it belongs to
    whichever deployment supplies the tools, and naming one in shared code
    would make every other deployment's agent call a tool that does not exist.
    """
    # Local import: the seam is overlayable, and a module-scope import would
    # bind CE's version before an overlay could replace it.
    from pi_dash.ee.cloud_agent.toolsets import extra_toolsets_schema_tool

    enabled = bool((getattr(run, "tool_plan", {}) or {}).get("extra_toolsets"))
    return {
        "extra_toolsets": enabled,
        "extra_toolsets_schema_tool": extra_toolsets_schema_tool() if enabled else "",
    }


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
    assignees = [(u.display_name or u.email or "") for u in issue.assignees.all()]
    project_states = [
        {
            "name": s.name,
            "group": s.group,
            "description": s.description or "",
        }
        for s in State.objects.filter(project=project)
    ]

    attempt = _compute_attempt(issue, run)

    # Walk the parent chain once: [issue, parent, grandparent, ... root].
    ancestors = _ancestor_chain(issue)

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
        "repo": _repo_context(project, issue),
        # Git PRs / code reviews already attached to this issue (empty list
        # when none). Lets the template tell the agent about associated PRs so
        # it can build on prior work and avoid opening a duplicate review.
        "code_reviews": _code_reviews_context(issue),
        "parent": (
            {
                "identifier": _issue_identifier(parent),
                "title": parent.name or "",
                "state": (parent.state.name if getattr(parent, "state", None) else ""),
                "work_branch": (getattr(parent, "git_work_branch", "") or None),
                "description": _issue_description_markdown(parent),
                "comments_count": _issue_comment_count(parent),
            }
            if parent is not None
            else None
        ),
        # Multi-level lineage (current -> parent -> ... -> root). Only set when
        # there's a grandparent or higher: for a single parent the `parent`
        # block already carries everything, so the template renders the lineage
        # tree only when ``lineage`` is truthy. We do NOT inline ancestor
        # content beyond the direct parent — the agent is told to fetch it via
        # the CLI on demand.
        "lineage": (
            [{"identifier": _issue_identifier(node), "title": node.name or ""} for node in ancestors]
            if len(ancestors) > 2
            else None
        ),
        # Direct children (one level down) and `relates_to` siblings, rendered
        # together with the ancestor chain in the "Work item relationships"
        # section. Empty lists when the issue has none — the template omits the
        # corresponding group so no empty heading or dangling directive renders.
        "children": _children_context(issue),
        "related": _related_context(issue),
        # Directional relations (blocked_by / blocking / start / finish /
        # implements), both link directions resolved to this item's point of
        # view, plus ``open_blockers`` / ``has_open_blockers`` for the
        # "Blocked by" warning. The agent — not dispatch — decides whether an
        # open blocker means it should wait (PDASHOSS01-195).
        **_relations_context(issue),
        "run": {
            "id": str(run.id),
            "kind": _issue_run_kind(issue),
            "attempt": attempt,
            "turn_number": 1,
            # How this run was dispatched, from the first-class
            # ``AgentRun.trigger`` field: "tick" / "comment_and_run" /
            # "run_ai" / "state_transition" / "scheduler" / "direct".
            # getattr, not attribute access: the template-preview endpoint
            # renders with a stub run that has no ``trigger``.
            "trigger": getattr(run, "trigger", None),
            "executor_kind": getattr(run, "executor_kind", "local_runner"),
        },
        "available_tools": (getattr(run, "tool_plan", {}) or {}).get("tools", []),
        "unavailable_capabilities": (getattr(run, "tool_plan", {}) or {}).get("unavailable_capabilities", []),
        **extra_toolsets_vars(run),
        "limits": (getattr(run, "tool_plan", {}) or {}).get("limits", {}),
        # Ticking schedule (None when the issue has no ticker row). Lets the
        # template explain the re-invocation cadence and remaining budget.
        "tick": _tick_context(issue),
        "comments_section": _comments_section(issue),
        "parent_done_payload": _parent_done_payload(issue, run),
        # Prior-run workpad body (empty on first run). Surfaced up front so
        # continuation runs see their predecessor's plan/phase/notes without
        # an extra ``pidash workpad get`` round-trip.
        "workpad_body": issue.workpad or "",
    }


def build_scheduler_task_body(binding) -> str:
    """Assemble the operator-authored task content for a scheduler run.

    Injected into the ``scheduler-task`` section as the ``scheduler_task_body``
    context variable — it is **never parsed as Jinja**, matching how issue
    descriptions / comments flow through the renderer. Order preserves the
    legacy dispatch concatenation: scheduler prompt, per-install extra context,
    then the per-binding outcome-mode work directive.
    """
    from pi_dash.db.models.scheduler import outcome_mode_directive

    scheduler = binding.scheduler
    parts = [
        ((getattr(scheduler, "prompt", "") or "").strip()),
        ((binding.extra_context or "").strip()),
        outcome_mode_directive(binding.outcome_mode),
    ]
    return "\n\n".join(p for p in parts if p)


def build_scheduler_context(binding, run: AgentRun) -> Dict[str, Any]:
    """Build the Jinja context for a project-scoped scheduler run.

    Issue-centric keys do not exist here; the base-context contract guarantees
    ``workspace``, ``project``, and ``run`` (with ``run.kind == "scheduler"``)
    so shared sections can branch safely. See design §5.2.
    """
    project = binding.project
    workspace = binding.workspace
    scheduler = binding.scheduler
    scheduler_task_body = build_scheduler_task_body(binding)
    if getattr(run, "executor_kind", "local_runner") == "cloud_agent":
        scheduler_task_body = "\n\n".join(
            part
            for part in [
                (getattr(scheduler, "prompt", "") or "").strip(),
                (binding.extra_context or "").strip(),
                (
                    "Search for duplicates and create at most one Pi Dash backlog issue "
                    "for the most important new finding."
                ),
            ]
            if part
        )
    return {
        "workspace": {
            "slug": getattr(workspace, "slug", ""),
            "name": getattr(workspace, "name", ""),
        },
        "project": {
            "id": str(project.id) if project is not None else "",
            "identifier": getattr(project, "identifier", ""),
            "name": getattr(project, "name", ""),
            "description": (getattr(project, "description", "") or ""),
        },
        "scheduler": {
            "slug": getattr(scheduler, "slug", ""),
            "name": getattr(scheduler, "name", ""),
            "description": (getattr(scheduler, "description", "") or ""),
        },
        "run": {
            "id": str(run.id),
            "kind": "scheduler",
            "attempt": 1,
            "turn_number": 1,
            "executor_kind": getattr(run, "executor_kind", "local_runner"),
        },
        "available_tools": (getattr(run, "tool_plan", {}) or {}).get("tools", []),
        "unavailable_capabilities": (getattr(run, "tool_plan", {}) or {}).get("unavailable_capabilities", []),
        **extra_toolsets_vars(run),
        "limits": (getattr(run, "tool_plan", {}) or {}).get("limits", {}),
        "scheduler_task_body": scheduler_task_body,
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
