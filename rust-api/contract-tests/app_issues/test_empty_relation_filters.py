"""Empty/null relation ``filters=`` leaves must not INNER JOIN (PIDASHCONV-1716).

``compile_leaf`` renders an empty relation value (``{"assignee_id":""}``,
``{"label_id__in":""}``, ``{"assignee_id":null}``) as the match-all
constant ``1 = 1``. The list engine decides whether a leaf "really
filters" through its relation join by checking that constant; before the
fix the check matched only ``SimpleExpr::Custom`` while the constants had
moved to ``SimpleExpr::Keyword``, so the empty leaf promoted the join to
INNER and unassigned/unlabelled issues vanished (``total_count`` 1
instead of 2). Django adds no join for an empty leaf (``Filter.filter``
returns the queryset untouched), so every issue stays in the list.
"""

import json
import uuid

import pytest


def _filtered(client, slug, project_id, tree):
    url = f"/api/workspaces/{slug}/projects/{project_id}/issues/"
    return client.get(url, params={"filters": json.dumps(tree)})


@pytest.fixture()
def pair(seed, db_conn):
    """Two seeded issues in project IS2: J1 gains an assignee and a label.

    IS2 (seed["project2"]) holds exactly J1 (admin-owned, bare) and J2
    (guest-owned, bare), so the empty-leaf list must return both. The
    fixture attaches a member assignee and a fresh label to J1 and
    removes them again on teardown.
    """
    label_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO labels (created_at, updated_at, id, name,"
            " description, created_by_id, project_id, updated_by_id,"
            " workspace_id, color, sort_order)"
            " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s, %s, %s)",
            (
                label_id,
                "EL label",
                "",
                seed["admin"],
                seed["project2"],
                seed["admin"],
                seed["ws"],
                "#ff0000",
                0.0,
            ),
        )
        cur.execute(
            "INSERT INTO issue_labels (created_at, updated_at, id,"
            " created_by_id, issue_id, label_id, project_id,"
            " updated_by_id, workspace_id)"
            " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s)",
            (
                str(uuid.uuid4()),
                seed["admin"],
                seed["j1"],
                label_id,
                seed["project2"],
                seed["admin"],
                seed["ws"],
            ),
        )
        cur.execute(
            "INSERT INTO issue_assignees (created_at, updated_at, id,"
            " assignee_id, created_by_id, issue_id, project_id,"
            " updated_by_id, workspace_id)"
            " VALUES (NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s)",
            (
                str(uuid.uuid4()),
                seed["member"],
                seed["admin"],
                seed["j1"],
                seed["project2"],
                seed["admin"],
                seed["ws"],
            ),
        )
    try:
        yield {"related": seed["j1"], "bare": seed["j2"], "project": seed["project2"]}
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "DELETE FROM issue_assignees WHERE issue_id = %s",
                (seed["j1"],),
            )
            cur.execute(
                "DELETE FROM issue_labels WHERE issue_id = %s",
                (seed["j1"],),
            )
            cur.execute("DELETE FROM labels WHERE id = %s", (label_id,))


def test_empty_assignee_filter_returns_all(clients, seed, pair):
    admin = clients["admin"]
    resp = _filtered(admin, seed["ws_slug"], pair["project"], {"assignee_id": ""})
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert {row["id"] for row in body["results"]} == {pair["related"], pair["bare"]}
    assert body["total_count"] == 2


def test_empty_label_in_filter_returns_all(clients, seed, pair):
    admin = clients["admin"]
    resp = _filtered(
        admin, seed["ws_slug"], pair["project"], {"label_id__in": ""}
    )
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert {row["id"] for row in body["results"]} == {pair["related"], pair["bare"]}
    assert body["total_count"] == 2


def test_null_assignee_filter_returns_all(clients, seed, pair):
    admin = clients["admin"]
    resp = _filtered(
        admin, seed["ws_slug"], pair["project"], {"assignee_id": None}
    )
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert {row["id"] for row in body["results"]} == {pair["related"], pair["bare"]}
    assert body["total_count"] == 2


def test_filled_assignee_filter_returns_matching_only(clients, seed, pair):
    admin = clients["admin"]
    resp = _filtered(
        admin, seed["ws_slug"], pair["project"], {"assignee_id": seed["member"]}
    )
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert {row["id"] for row in body["results"]} == {pair["related"]}
    assert body["total_count"] == 1
