# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Agent-facing issue relations — create, remove and list (PDASHOSS01-199).

One implementation behind every agent surface (``pidash issue relate`` /
``unrelate`` / ``relations`` via the v1 API, the chat assistant's
``relate_issues`` / ``unrelate_issues`` / ``list_issue_relations``, the Cloud
Agent's ``pidash_*`` equivalents, and the hosted MCP connector) so the agent
sees one vocabulary and one output shape everywhere:

- Relation types are named from the *source* issue's point of view:
  ``blocked_by``, ``blocking``, ``relates_to``, ``duplicate``,
  ``start_before``, ``start_after``, ``finish_before``, ``finish_after``,
  ``implemented_by``, ``implements``. ``relate(A, "blocking", [B])`` stores
  the same row as ``relate(B, "blocked_by", [A])``.
- Writes are idempotent. Relating a pair that already carries the requested
  relation is reported under ``unchanged``; removing one that is absent is
  reported under ``not_related``. Neither is an error.
- A pair holds at most one live relation (the table's unique constraint is
  per pair, whatever the type). Asking for a different relation on a pair
  that already has one is reported under ``conflicts`` with the existing
  relation, and nothing is overwritten: the agent unrelates first if it
  really means to change it. This also refuses the direct 2-cycle
  "A blocked_by B" when "B blocked_by A" exists.
- :func:`grouped_relations` returns ``{type: [item, ...]}`` for every type
  above, each item ``{id, identifier, name, state, state_group}`` — a
  superset of the ``relations_summary`` item from
  :mod:`pi_dash.orchestration.blockers`. Items the viewer cannot see (a
  project they are not an active member of) are left out.

Access control is the caller's job: resolve the source issue and the targets
through the caller's own scoping (DRF permission + ``member_project_issues``,
the assistant's ``_scoping``, the Cloud Agent's run scope), then hand the
resolved rows here.
"""

from __future__ import annotations

import json
import uuid
from typing import Any, Dict, Iterable, List, Optional

from django.core.serializers.json import DjangoJSONEncoder
from django.db import IntegrityError, transaction
from django.db.models import Q, QuerySet
from django.utils import timezone

from pi_dash.db.models import Issue, IssueRelation
from pi_dash.utils.issue_relation_mapper import get_actual_relation, get_inverse_relation

#: Every relation type an agent may name, in display order. Pairs sit next to
#: each other: the second of each pair is the first seen from the other end.
RELATION_TYPES = (
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
)

#: Types stored with the ends swapped under their forward name
#: (``blocking`` is written as ``blocked_by`` on the other issue).
REVERSE_TYPES = frozenset({"blocking", "start_after", "finish_after", "implements"})

#: Per-type cap on :func:`grouped_relations` lists; matches
#: ``blockers.SUMMARY_LIMIT`` so the issue read path stays light.
GROUP_LIMIT = 100


class RelationError(ValueError):
    """Invalid relation request (unknown type, self-relation)."""


def validate_relation_type(relation_type: str) -> str:
    value = (relation_type or "").strip().lower()
    if value not in RELATION_TYPES:
        raise RelationError(f"relation_type must be one of: {', '.join(RELATION_TYPES)}")
    return value


def identifier(issue: Issue) -> str:
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _stored_edge(source_id, relation_type: str, target_id):
    """``(issue_id, related_issue_id, stored_type)`` for "source <type> target"."""
    stored = get_actual_relation(relation_type)
    if relation_type in REVERSE_TYPES:
        return target_id, source_id, stored
    return source_id, target_id, stored


def _type_from(row: IssueRelation, viewpoint_id) -> str:
    """The relation ``row`` expresses, named from ``viewpoint_id``'s side.

    Rows stored under a reverse name (``blocking``) — which the UI never
    writes but older data may hold — are normalised like
    :mod:`pi_dash.orchestration.blockers` does.
    """
    stored = row.relation_type
    issue_id, related_id = row.issue_id, row.related_issue_id
    if stored in REVERSE_TYPES:
        stored = get_actual_relation(stored)
        issue_id, related_id = related_id, issue_id
    if str(issue_id) == str(viewpoint_id):
        return stored
    return get_inverse_relation(stored)


def _pair_rows(a_id, b_id) -> QuerySet:
    return IssueRelation.objects.filter(
        Q(issue_id=a_id, related_issue_id=b_id) | Q(issue_id=b_id, related_issue_id=a_id)
    )


def _log_activity(activity_type: str, requested: dict, issue: Issue, actor, current_instance=None) -> None:
    from pi_dash.bgtasks.issue_activities_task import issue_activity

    issue_activity.delay(
        type=activity_type,
        requested_data=json.dumps(requested, cls=DjangoJSONEncoder),
        actor_id=str(actor.id),
        issue_id=str(issue.id),
        project_id=str(issue.project_id),
        current_instance=current_instance,
        epoch=int(timezone.now().timestamp()),
        notification=True,
    )


def resolve_refs(refs: Iterable[str], issues: QuerySet) -> tuple[List[Issue], List[str]]:
    """Resolve issue references (``PROJ-123`` or UUID) against ``issues``.

    ``issues`` is the caller's already-scoped queryset (e.g. the viewer's
    ``member_project_issues``), so a reference the caller may not see is
    indistinguishable from one that does not exist. Returns ``(found,
    unresolved)`` with ``found`` in request order.
    """
    found: List[Issue] = []
    unresolved: List[str] = []
    for raw in refs:
        ref = str(raw or "").strip()
        match = None
        if ref:
            try:
                match = issues.filter(id=uuid.UUID(ref))
            except ValueError:
                project_ident, sep, seq = ref.rpartition("-")
                if sep and project_ident and seq.isdigit():
                    match = issues.filter(project__identifier__iexact=project_ident, sequence_id=int(seq))
        issue = match.select_related("project", "state").first() if match is not None else None
        if issue is None:
            unresolved.append(ref or str(raw))
        else:
            found.append(issue)
    return found, unresolved


def _check_targets(issue: Issue, targets: Iterable[Issue]) -> List[Issue]:
    unique: List[Issue] = []
    seen = set()
    for target in targets:
        if target.id == issue.id:
            raise RelationError(f"{identifier(issue)} cannot be related to itself")
        if target.workspace_id != issue.workspace_id:
            raise RelationError(f"{identifier(target)} is in a different workspace")
        if target.id not in seen:
            seen.add(target.id)
            unique.append(target)
    return unique


def relate(issue: Issue, relation_type: str, targets: Iterable[Issue], actor) -> Dict[str, Any]:
    """Record "``issue`` <relation_type> each target". Idempotent.

    Returns ``{issue, relation_type, created, unchanged, conflicts}``;
    ``created`` / ``unchanged`` are identifier lists, ``conflicts`` a list of
    ``{identifier, existing_relation}`` (``existing_relation`` named from
    ``issue``'s side).
    """
    relation_type = validate_relation_type(relation_type)
    targets = _check_targets(issue, targets)
    created: List[Issue] = []
    unchanged: List[str] = []
    conflicts: List[Dict[str, str]] = []
    for target in targets:
        issue_id, related_id, stored = _stored_edge(issue.id, relation_type, target.id)
        existing = list(_pair_rows(issue.id, target.id))
        if not existing:
            try:
                with transaction.atomic():
                    IssueRelation.objects.create(
                        issue_id=issue_id,
                        related_issue_id=related_id,
                        relation_type=stored,
                        project_id=issue.project_id,
                        workspace_id=issue.workspace_id,
                        created_by=actor,
                        updated_by=actor,
                    )
            except IntegrityError:
                # A concurrent writer created the pair first; re-read it below.
                existing = list(_pair_rows(issue.id, target.id))
            else:
                created.append(target)
                continue
        current = {_type_from(row, issue.id) for row in existing}
        if current == {relation_type}:
            unchanged.append(identifier(target))
        else:
            conflicts.append({"identifier": identifier(target), "existing_relation": sorted(current)[0]})
    if created:
        _log_activity(
            "issue_relation.activity.created",
            {"relation_type": relation_type, "issues": [str(t.id) for t in created]},
            issue,
            actor,
        )
    return {
        "issue": identifier(issue),
        "relation_type": relation_type,
        "created": [identifier(t) for t in created],
        "unchanged": unchanged,
        "conflicts": conflicts,
    }


def unrelate(issue: Issue, relation_type: str, targets: Iterable[Issue], actor) -> Dict[str, Any]:
    """Remove "``issue`` <relation_type> each target". Idempotent.

    Only a relation of exactly that type is removed — ``unrelate(A,
    "blocked_by", [B])`` leaves an ``A relates_to B`` alone and reports B under
    ``not_related``. Returns ``{issue, relation_type, removed, not_related}``.
    """
    relation_type = validate_relation_type(relation_type)
    targets = _check_targets(issue, targets)
    removed: List[str] = []
    not_related: List[str] = []
    for target in targets:
        rows = [row for row in _pair_rows(issue.id, target.id) if _type_from(row, issue.id) == relation_type]
        if not rows:
            not_related.append(identifier(target))
            continue
        for row in rows:
            row.delete()
        removed.append(identifier(target))
        _log_activity(
            "issue_relation.activity.deleted",
            {"relation_type": relation_type, "related_issue": str(target.id)},
            issue,
            actor,
            current_instance=json.dumps({"relation_type": relation_type}),
        )
    return {
        "issue": identifier(issue),
        "relation_type": relation_type,
        "removed": removed,
        "not_related": not_related,
    }


def _item(issue: Issue) -> Dict[str, Any]:
    state = issue.state
    return {
        "id": str(issue.id),
        "identifier": identifier(issue),
        "name": issue.name,
        "state": state.name if state else None,
        "state_group": state.group if state else None,
    }


def grouped_relations(issue: Issue, visible_issues: Optional[QuerySet] = None) -> Dict[str, List[Dict[str, Any]]]:
    """Every live relation of ``issue``, grouped by type from its side.

    ``visible_issues`` narrows the other ends to what the viewer may see
    (e.g. ``member_project_issues(user, slug)``); ``None`` means any live
    work item in the workspace. Every type key is always present.
    """
    rows = IssueRelation.objects.filter(
        Q(issue_id=issue.id) | Q(related_issue_id=issue.id),
        workspace_id=issue.workspace_id,
    ).exclude(issue_id=issue.id, related_issue_id=issue.id)
    other_by_type: Dict[str, set] = {t: set() for t in RELATION_TYPES}
    for row in rows:
        other = row.related_issue_id if row.issue_id == issue.id else row.issue_id
        relation = _type_from(row, issue.id)
        if relation in other_by_type:
            other_by_type[relation].add(other)

    wanted = set().union(*other_by_type.values())
    pool = visible_issues if visible_issues is not None else Issue.issue_objects.all()
    others = {
        i.id: i
        for i in pool.filter(id__in=wanted, workspace_id=issue.workspace_id)
        .select_related("state", "project")
        .order_by()
    }

    def sort_key(i: Issue):
        return (i.project.identifier, i.sequence_id)

    return {
        relation: [_item(i) for i in sorted((others[o] for o in ids if o in others), key=sort_key)][:GROUP_LIMIT]
        for relation, ids in other_by_type.items()
    }
