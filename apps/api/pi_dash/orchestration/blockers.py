# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Blocker lookups — the single place "what is this work item blocked by, and
who is waiting on it?" is answered.

Informational only: nothing here gates dispatch. A human moving an issue to
In Progress always gets a run; the agent reads its blockers (prompt, issue
read APIs, ``pidash issue get``) and decides whether to proceed or wait
(PDASHOSS01-195).

Two forms, mirroring ``pi_dash.loop.eligibility``:

- row form (:func:`open_blockers`, :func:`blockers`, :func:`dependents`,
  :func:`relations_summary`) for one issue;
- queryset form (:func:`open_blockers_q`) — an ``Exists`` predicate for bulk
  scans over an ``Issue`` queryset without an N+1.

Both share :func:`_blocked_by_edges` / :func:`_blocking_edges`, so they cannot
disagree about what counts as a blocker:

- A row ``(issue=A, related_issue=B, relation_type="blocked_by")`` means "A is
  blocked by B". The UI only ever stores the forward type (``blocking`` is
  written as ``blocked_by`` with the ends swapped, see
  ``utils.issue_relation_mapper.get_actual_relation``), but a row stored under
  the reverse name from ``IssueRelationChoices._REVERSE_MAPPING``
  (``(B, A, "blocking")``) is read the same way.
- Soft-deleted relation rows are ignored.
- The other end must be a live work item (``Issue.issue_objects``: not
  deleted, archived, draft or triage) in the relation's workspace — the same
  set the relation view lists. Cross-project targets are included.
- A blocker is *open* until its state group is ``completed`` or
  ``cancelled``; ``review`` / ``test`` still count as open. A blocker with no
  state is open.
"""

from __future__ import annotations

from typing import Any, Dict, List

from django.db.models import Case, Exists, F, IntegerField, OuterRef, Q, QuerySet, Value, When

from pi_dash.db.models.issue import Issue, IssueRelation, IssueRelationChoices
from pi_dash.db.models.state import StateGroup

BLOCKED_BY = IssueRelationChoices.BLOCKED_BY.value
BLOCKING = IssueRelationChoices._REVERSE_MAPPING[BLOCKED_BY]

#: State groups that mean a blocker no longer holds its dependents back.
CLOSED_STATE_GROUPS = frozenset({StateGroup.COMPLETED.value, StateGroup.CANCELLED.value})

#: Per-direction cap on :func:`relations_summary` lists. The summary is meant
#: to stay light on the issue read path; a widely depended-on issue can have
#: hundreds of dependents. Open items sort first so they are never the ones
#: cut, and ``has_open_blockers`` is always computed over the full set.
SUMMARY_LIMIT = 100


def _live_relations() -> QuerySet:
    return IssueRelation.objects.filter(deleted_at__isnull=True).exclude(issue_id=F("related_issue_id"))


def _blocked_by_edges(dependent, targets: QuerySet) -> tuple[QuerySet, QuerySet]:
    """Relation rows saying ``dependent`` is blocked by an issue in ``targets``.

    ``dependent`` is an issue id or an ``OuterRef``. Returns ``(forward,
    stored_reversed)``: in ``forward`` the blocker is ``related_issue``, in
    ``stored_reversed`` it is ``issue``.
    """
    rels = _live_relations()
    forward = rels.filter(
        issue_id=dependent,
        relation_type=BLOCKED_BY,
        related_issue__in=targets,
        related_issue__workspace_id=F("workspace_id"),
    )
    stored_reversed = rels.filter(
        related_issue_id=dependent,
        relation_type=BLOCKING,
        issue__in=targets,
        issue__workspace_id=F("workspace_id"),
    )
    return forward, stored_reversed


def _blocking_edges(blocker, targets: QuerySet) -> tuple[QuerySet, QuerySet]:
    """Relation rows saying ``blocker`` blocks an issue in ``targets``.

    Mirror of :func:`_blocked_by_edges`: in ``forward`` the dependent is
    ``issue``, in ``stored_reversed`` it is ``related_issue``.
    """
    rels = _live_relations()
    forward = rels.filter(
        related_issue_id=blocker,
        relation_type=BLOCKED_BY,
        issue__in=targets,
        issue__workspace_id=F("workspace_id"),
    )
    stored_reversed = rels.filter(
        issue_id=blocker,
        relation_type=BLOCKING,
        related_issue__in=targets,
        related_issue__workspace_id=F("workspace_id"),
    )
    return forward, stored_reversed


def _open(issues: QuerySet) -> QuerySet:
    return issues.exclude(state__group__in=CLOSED_STATE_GROUPS)


def blockers_queryset(issue: Issue) -> QuerySet:
    """Every live ``blocked_by`` target of ``issue``, open or resolved."""
    forward, stored_reversed = _blocked_by_edges(issue.id, Issue.issue_objects.all())
    return Issue.issue_objects.filter(
        Q(id__in=forward.values("related_issue_id")) | Q(id__in=stored_reversed.values("issue_id"))
    )


def dependents_queryset(issue: Issue) -> QuerySet:
    """Every live issue that lists ``issue`` under ``blocked_by``, any state."""
    forward, stored_reversed = _blocking_edges(issue.id, Issue.issue_objects.all())
    return Issue.issue_objects.filter(
        Q(id__in=forward.values("issue_id")) | Q(id__in=stored_reversed.values("related_issue_id"))
    )


def _ordered(issues: QuerySet) -> QuerySet:
    return issues.select_related("state", "project").order_by("project__identifier", "sequence_id")


def blockers(issue: Issue) -> List[Issue]:
    """``blocked_by`` targets of ``issue`` in any state."""
    return list(_ordered(blockers_queryset(issue)))


def open_blockers(issue: Issue) -> List[Issue]:
    """``blocked_by`` targets of ``issue`` not yet completed or cancelled."""
    return list(_ordered(_open(blockers_queryset(issue))))


def has_open_blockers(issue: Issue) -> bool:
    return _open(blockers_queryset(issue)).exists()


def dependents(issue: Issue) -> List[Issue]:
    """Issues blocked by ``issue`` (reverse of :func:`blockers`), any state."""
    return list(_ordered(dependents_queryset(issue)))


def open_blockers_q(issue_ref: str = "pk") -> Q:
    """Predicate: does the issue at ``OuterRef(issue_ref)`` have an open blocker?

    For bulk scans: ``Issue.objects.filter(open_blockers_q())``, or from a
    model that points at an issue, ``IssueAgentTicker.objects.exclude(
    open_blockers_q("issue_id"))``. Same rules as :func:`open_blockers`.
    """
    forward, stored_reversed = _blocked_by_edges(OuterRef(issue_ref), _open(Issue.issue_objects.all()))
    return Q(Exists(forward)) | Q(Exists(stored_reversed))


def _summary_item(issue: Issue) -> Dict[str, Any]:
    state = issue.state
    return {
        "identifier": f"{issue.project.identifier}-{issue.sequence_id}",
        "state": state.name if state else None,
        "state_group": state.group if state else None,
    }


def _summary_list(issues: QuerySet) -> List[Dict[str, Any]]:
    open_first = issues.annotate(
        _resolved=Case(
            When(state__group__in=CLOSED_STATE_GROUPS, then=Value(1)),
            default=Value(0),
            output_field=IntegerField(),
        )
    )
    rows = open_first.select_related("state", "project").order_by("_resolved", "project__identifier", "sequence_id")
    return [_summary_item(i) for i in rows[:SUMMARY_LIMIT]]


def relations_summary(issue: Issue) -> Dict[str, Any]:
    """Light blocker summary for issue read paths (API + ``pidash issue get``).

    ``{"relations_summary": {"blocked_by": [...], "blocking": [...]},
    "has_open_blockers": bool}`` where each item is ``{identifier, state,
    state_group}`` — no titles or bodies. Lists put open items first and are
    capped at :data:`SUMMARY_LIMIT`.
    """
    return {
        "relations_summary": {
            "blocked_by": _summary_list(blockers_queryset(issue)),
            "blocking": _summary_list(dependents_queryset(issue)),
        },
        "has_open_blockers": has_open_blockers(issue),
    }
