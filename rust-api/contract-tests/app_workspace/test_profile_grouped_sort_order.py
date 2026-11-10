# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-154: grouped profile user-issues rows must render sort_order as
a JSON number, not a JSON string.

Django's ``Issue.sort_order`` is a ``FloatField`` and the grouped branch
serializes it through the issue serializer, so every row carries e.g.
``"sort_order": 105535.0``. The Rust grouped path shaped ``sort_order`` to a
Python-repr string for the grouping kernels and never unquoted it, so rows
arrived as ``"sort_order": "105535.0"`` and the new frontend's
``sort_order: z.number()`` schema refused the whole grouped response.

Every ``group_by`` (state_id, priority, labels__id, assignees__id,
issue_module__module_id, created_by, ...) and the sub-grouped path flow
through the same ``results_json`` splice, so the state_id grouped and
state_id/priority sub-grouped cases pin the fix. The flat list splices the
same text bare (number) and is unaffected; it is pinned by
``test_workspace_extras.py::test_user_issues_shape``.
"""

import pytest

from _harness import env
from _harness.seed import Seed


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


def _seed_profile_issues(world, space):
    """Two states, three issues created by the caller, distinct sort_orders.

    Returns ``(issue_ids, expected_sort_orders)``.
    """
    conn = env.connect()
    try:
        seed = Seed(conn)
        uid, ws_id = world["user"]["id"], space["id"]
        project_id = seed.project(ws_id)
        seed.project_member(project_id, ws_id, uid, role=20)
        state_a = seed.state(project_id, ws_id, name="Profile Todo",
                             group="unstarted")
        state_b = seed.state(project_id, ws_id, name="Profile Doing",
                             group="started")
        issue_a1 = seed.issue(ws_id, project_id, name="Grouped A1",
                              state_id=state_a)
        issue_a2 = seed.issue(ws_id, project_id, name="Grouped A2",
                              state_id=state_a)
        issue_b1 = seed.issue(ws_id, project_id, name="Grouped B1",
                              state_id=state_b)
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET created_by_id = %s WHERE id IN (%s, %s, %s)",
                (uid, issue_a1, issue_a2, issue_b1),
            )
            # Distinct values pin per-row rendering, incl. a fractional
            # offset (dragover sort_orders are not always integral).
            cur.execute(
                "UPDATE issues SET sort_order = 105535.0 WHERE id = %s",
                (issue_a1,),
            )
            cur.execute(
                "UPDATE issues SET sort_order = 12345.5 WHERE id = %s",
                (issue_b1,),
            )
    finally:
        conn.close()
    return (
        {issue_a1, issue_a2, issue_b1},
        {issue_a1: 105535.0, issue_a2: 65535.0, issue_b1: 12345.5},
    )


def _grouped_rows(body):
    rows = []
    for _group, cell in body["results"].items():
        rows.extend(cell["results"])
    return rows


def _sub_grouped_rows(body):
    rows = []
    for _group, cell in body["results"].items():
        for _sub, sub_cell in cell["results"].items():
            rows.extend(sub_cell["results"])
    return rows


def _assert_sort_order_numbers(rows, expected_ids, expected_values):
    assert {row["id"] for row in rows} == expected_ids
    for row in rows:
        value = row["sort_order"]
        assert isinstance(value, (int, float)) and not isinstance(value, bool), (
            f"sort_order must be a JSON number, got {type(value).__name__}: "
            f"{value!r} for issue {row['id']}"
        )
        assert value == expected_values[row["id"]]


def test_grouped_sort_order_is_number(user_api, space, world):
    expected_ids, expected_values = _seed_profile_issues(world, space)
    url = f"/api/workspaces/{space['slug']}/user-issues/{world['user']['id']}/"
    res = user_api.get(
        url,
        params={"group_by": "state_id", "order_by": "-created_at",
                "per_page": 100},
    )
    assert res.status_code == 200, res.text
    body = res.json()
    assert body["grouped_by"] == "state_id"
    assert body["sub_grouped_by"] is None
    _assert_sort_order_numbers(_grouped_rows(body),
                                expected_ids, expected_values)


def test_sub_grouped_sort_order_is_number(user_api, space, world):
    expected_ids, expected_values = _seed_profile_issues(world, space)
    url = f"/api/workspaces/{space['slug']}/user-issues/{world['user']['id']}/"
    res = user_api.get(
        url,
        params={"group_by": "state_id", "sub_group_by": "priority",
                "order_by": "-created_at", "per_page": 100},
    )
    assert res.status_code == 200, res.text
    body = res.json()
    assert body["grouped_by"] == "state_id"
    assert body["sub_grouped_by"] == "priority"
    _assert_sort_order_numbers(_sub_grouped_rows(body),
                                expected_ids, expected_values)
