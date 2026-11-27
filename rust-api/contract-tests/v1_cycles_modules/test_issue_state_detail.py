"""Contract tests for PDASHOSS01-404: ``state_detail`` on the cycle and module
issue lists.

Both lists render rows through the v1 ``IssueSerializer``, which carries
``state_detail: {name, group}`` (``null`` without a state) next to the bare
``state`` UUID. ``?fields=state_detail`` keeps only that object, and the
single cycle-issue read with ``?expand=issue`` renders the same field on the
nested issue.
"""

import pytest

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

CYCLE_LIST = api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID)
MODULE_LIST = api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)

STARTED = {"name": "CT Started", "group": "started"}
COMPLETED = {"name": "CT Completed", "group": "completed"}
UNSTARTED = {"name": "CT Unstarted", "group": "unstarted"}

# (label, list url, {issue id: expected state_detail}) — the fixed seed rows.
CASES = [
    (
        "cycle",
        CYCLE_LIST,
        {db.ISSUE_STARTED_ID: STARTED, db.ISSUE_COMPLETED_ID: COMPLETED},
    ),
    ("module", MODULE_LIST, {db.ISSUE_UNSTARTED_ID: UNSTARTED}),
]


def _rows(response):
    assert response.status_code == 200, response.text
    return response.json()["results"]


@pytest.mark.parametrize("label,url,expected", CASES)
def test_rows_carry_state_detail(admin_client, label, url, expected):
    rows = _rows(admin_client.get(url))
    assert {row["id"]: row["state_detail"] for row in rows} == expected
    for row in rows:
        keys = list(row)
        # Declared after ``url``, before the model fields.
        assert keys.index("state_detail") < keys.index("created_at"), keys


@pytest.mark.parametrize("label,url,expected", CASES)
def test_fields_state_detail_returns_only_that_object(admin_client, label, url, expected):
    rows = _rows(admin_client.get(url, params={"fields": "state_detail"}))
    assert sorted(rows, key=lambda row: row["state_detail"]["name"]) == [
        {"state_detail": detail}
        for detail in sorted(expected.values(), key=lambda detail: detail["name"])
    ]


@pytest.mark.parametrize("label,url,expected", CASES)
def test_state_detail_is_null_without_a_state(admin_client, db_conn, label, url, expected):
    issue_id = next(iter(expected))
    with db_conn.cursor() as cur:
        cur.execute("UPDATE issues SET state_id = NULL WHERE id = %s", [issue_id])
    db_conn.commit()
    rows = _rows(admin_client.get(url, params={"fields": "id,state,state_detail"}))
    (row,) = [row for row in rows if row["id"] == issue_id]
    assert row == {"id": issue_id, "state_detail": None, "state": None}


def test_cycle_issue_expand_issue_carries_state_detail(admin_client):
    response = admin_client.get(
        api.cycle_issue_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID, db.ISSUE_STARTED_ID),
        params={"expand": "issue"},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["state_detail"] == STARTED
