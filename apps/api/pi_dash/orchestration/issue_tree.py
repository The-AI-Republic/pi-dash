# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""A work item's children with a progress rollup (PDASHOSS01-405).

Backs ``GET .../work-items/<id>/tree/`` and ``pidash issue tree``: one call
answers "how far along is this epic?" without paging every work item in the
project and grouping by parent client-side.

Shape::

    {
      "issue":     {id, identifier, name, state, state_group, priority},
      "recursive": bool,
      "rollup":    {total, done, open, by_state_group: {<group>: n, ...}},
      "children":  [{id, identifier, name, state, state_group, priority,
                     parent, depth, assignees: [{id, display_name}],
                     active_run: {id, status} | null}, ...],
      "truncated": bool,
    }

- ``children`` are the direct children (``depth`` 1), or with
  ``recursive`` every descendant, in depth-first pre-order with siblings by
  ``sequence_id``. ``parent`` is the identifier of the item's parent.
- ``rollup`` counts every listed child. ``done`` is the closed groups
  (``completed`` and ``cancelled``: a cancelled child is closed, not
  outstanding); ``open`` is ``total - done``, so a child without a state
  counts as open. ``by_state_group`` always carries every group in
  lifecycle order, zeros included.
- ``active_run`` is the newest non-terminal agent run on the child.
- Children the caller cannot see (a project they are not an active member
  of) and archived, draft, triage or deleted ones are left out, the same
  rows ``member_project_issues`` yields. A hidden child's subtree is cut
  off with it, even where a grandchild would be visible.
- At most :data:`NODE_LIMIT` children are listed and walked; past that
  ``truncated`` is true and the rollup covers only the listed ones. A parent
  cycle in the data cannot loop: each item is visited once.
"""

from __future__ import annotations

from typing import Any, Dict, List

from pi_dash.db.models import Issue, IssueAssignee
from pi_dash.utils.constants import CLOSED_STATE_GROUPS, STATE_GROUP_ORDER

#: Cap on listed children (all depths together).
NODE_LIMIT = 1000


def _identifier(issue: Issue) -> str:
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _header(issue: Issue) -> Dict[str, Any]:
    state = issue.state
    return {
        "id": str(issue.id),
        "identifier": _identifier(issue),
        "name": issue.name,
        "state": state.name if state else None,
        "state_group": state.group if state else None,
        "priority": issue.priority,
    }


def rollup(groups: List[Any]) -> Dict[str, Any]:
    """Counts over the state groups of the listed children."""
    by_group = {group: 0 for group in STATE_GROUP_ORDER}
    for group in groups:
        if group in by_group:
            by_group[group] += 1
    done = sum(by_group[group] for group in CLOSED_STATE_GROUPS)
    return {
        "total": len(groups),
        "done": done,
        "open": len(groups) - done,
        "by_state_group": by_group,
    }


def _descendants(issue: Issue, visible, *, recursive: bool):
    """Ids of the listed children in ``(depth, sequence_id, id)`` order, and
    whether the :data:`NODE_LIMIT` cut them short.

    One recursive query instead of one query per level, so a deep chain
    costs a single round trip. Only rows in ``visible`` are followed: a
    hidden child stops the walk below it. The source is never re-entered,
    which is the only way a parent cycle can be reached from it (every
    item has one parent), and depth is capped at ``NODE_LIMIT + 1``: a
    deeper row cannot rank inside the first ``NODE_LIMIT + 1`` rows, since
    each level above it contributes at least one row. Ordering by depth
    first keeps the earlier level-by-level result: every shallower level is
    listed in full before the cut falls inside one level, by sequence_id.
    """
    from django.db import connections

    pool_sql, pool_params = visible.order_by().values("id").query.sql_with_params()
    table = connections[visible.db].ops.quote_name(Issue._meta.db_table)
    sql = f"""
        WITH RECURSIVE tree (id, depth) AS (
            SELECT i.id, 1 FROM {table} i
            WHERE i.parent_id = %s AND i.id <> %s AND i.id IN ({pool_sql})
          UNION ALL
            SELECT i.id, t.depth + 1 FROM {table} i JOIN tree t ON i.parent_id = t.id
            WHERE %s AND t.depth <= %s AND i.id <> %s AND i.id IN ({pool_sql})
        )
        SELECT t.id FROM tree t JOIN {table} i ON i.id = t.id
        ORDER BY t.depth, i.sequence_id, i.id
        LIMIT %s
    """  # noqa: S608 - only the quoted table name and Django-compiled SQL are interpolated
    params = [issue.id, issue.id, *pool_params, recursive, NODE_LIMIT, issue.id, *pool_params, NODE_LIMIT + 1]
    with connections[visible.db].cursor() as cursor:
        cursor.execute(sql, params)
        ids = [row[0] for row in cursor.fetchall()]
    return ids[:NODE_LIMIT], len(ids) > NODE_LIMIT


def issue_tree(issue: Issue, visible, *, recursive: bool = False) -> Dict[str, Any]:
    """Build the tree payload for ``issue`` over the ``visible`` queryset."""
    from pi_dash.runner.models import AgentRun
    from pi_dash.runner.services.matcher import NON_TERMINAL_STATUSES

    rows, truncated = _descendants(issue, visible, recursive=recursive)
    by_id = {row.id: row for row in visible.select_related("project", "state").order_by().filter(id__in=rows)}
    nodes: List[tuple] = []  # (issue, parent identifier, depth)
    children_of: Dict[Any, List[Issue]] = {}
    # ``rows`` is in (depth, sequence_id, id) order, so each sibling list
    # comes out by sequence_id.
    for row_id in rows:
        row = by_id[row_id]
        children_of.setdefault(row.parent_id, []).append(row)

    # Depth-first pre-order with siblings by sequence_id. An explicit stack,
    # not recursion: a chain near NODE_LIMIT deep would exceed Python's
    # recursion limit.
    stack = [(child, issue, 1) for child in reversed(children_of.get(issue.id, []))]
    while stack:
        child, parent, level = stack.pop()
        nodes.append((child, _identifier(parent), level))
        stack.extend((grandchild, child, level + 1) for grandchild in reversed(children_of.get(child.id, [])))

    ids = [node.id for node, _, _ in nodes]
    assignees: Dict[Any, List[Dict[str, Any]]] = {}
    for row in (
        IssueAssignee.objects.filter(issue_id__in=ids, deleted_at__isnull=True)
        .select_related("assignee")
        .order_by("created_at", "id")
    ):
        assignees.setdefault(row.issue_id, []).append(
            {"id": str(row.assignee_id), "display_name": row.assignee.display_name}
        )
    active: Dict[Any, Dict[str, Any]] = {}
    for run in (
        AgentRun.objects.filter(work_item_id__in=ids, status__in=NON_TERMINAL_STATUSES)
        .order_by("-created_at", "-id")
        .only("id", "status", "work_item_id")
    ):
        active.setdefault(run.work_item_id, {"id": str(run.id), "status": run.status})

    children = []
    for node, parent_ident, level in nodes:
        item = _header(node)
        item["parent"] = parent_ident
        item["depth"] = level
        item["assignees"] = assignees.get(node.id, [])
        item["active_run"] = active.get(node.id)
        children.append(item)

    return {
        "issue": _header(issue),
        "recursive": recursive,
        "rollup": rollup([child["state_group"] for child in children]),
        "children": children,
        "truncated": truncated,
    }
