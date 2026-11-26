"""Contract: app issue routes on an archived project (DIFF-45, NEWPIDASH-236).

Rust answers 409 `project_archived` with identifier + archived_at on
every project-scoped app issue route after auth and the role gate.
Django hides inconsistently (200 empty list, 200 full detail, 500 create,
404 patch, `[]` comments).
"""

import pytest

from _harness.diffs import expect

pytestmark = pytest.mark.contract

ARCHIVED_AT = "2026-11-13T18:57:37+00:00"
ARCHIVED_AT_Z = "2026-11-13T18:57:37Z"


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _issue(ws, pid, iid):
    return f"{_base(ws, pid)}/issues/{iid}"


@pytest.fixture()
def archived_project(seed, db_conn):
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET archived_at = %s WHERE id = %s",
            (ARCHIVED_AT, seed["project"]),
        )
        cur.execute("SELECT identifier FROM projects WHERE id = %s", (seed["project"],))
        identifier = cur.fetchone()[0]
    yield identifier
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET archived_at = NULL WHERE id = %s", (seed["project"],)
        )


def _assert_archived_body(response, identifier):
    body = response.json()
    assert set(body) == {
        "error",
        "code",
        "project",
        "project_identifier",
        "archived_at",
    }, body
    assert body["code"] == "project_archived"
    assert body["project"] == identifier
    assert body["project_identifier"] == identifier
    assert body["archived_at"] == ARCHIVED_AT_Z
    assert body["error"] == (
        f"Project {identifier} is archived (since {ARCHIVED_AT_Z}). "
        "Unarchive it to read or change its work items."
    )


def test_archived_list_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_base(ws, pid)}/issues/list/?issues={i1}")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_detail_uuid_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_issue(ws, pid, i1)}/")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_list_by_identifier_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    response = admin.get(f"{_base(ws, archived_project)}/issues/list/?issues={seed['issue1']}")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_create_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    response = admin.post(f"{_base(ws, pid)}/issues/", json={"name": "Nope"})
    assert response.status_code == expect("DIFF-45", django=500, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_patch_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.patch(f"{_issue(ws, pid, i1)}/", json={"name": "Nope"})
    assert response.status_code == expect("DIFF-45", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_comments_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_issue(ws, pid, i1)}/comments/")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_identifier_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    response = admin.get(f"/api/workspaces/{ws}/work-items/{archived_project}-1/")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_identifier_lite_409(clients, seed, archived_project):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    response = admin.get(
        f"/api/workspaces/{ws}/work-items/{archived_project}-1/?lite=true"
    )
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_project)


def test_archived_bulk_archive_409(clients, seed, archived_project, db_conn):
    admin = clients["admin"]
    ws, pid, i3 = seed["ws_slug"], seed["project"], seed["issue3"]
    try:
        response = admin.post(
            f"{_base(ws, pid)}/bulk-archive-issues/", json={"issue_ids": [i3]}
        )
        assert response.status_code == expect("DIFF-45", django=200, rust=409)
        if response.status_code == 409:
            _assert_archived_body(response, archived_project)
            with db_conn.cursor() as cur:
                cur.execute(
                    "SELECT archived_at FROM issues WHERE id = %s", (i3,)
                )
                assert cur.fetchone()[0] is None
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET archived_at = NULL WHERE id = %s", (i3,)
            )


def test_active_list_unchanged(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_base(ws, pid)}/issues/list/?issues={i1}")
    assert response.status_code == 200, response.text


def test_active_detail_unchanged(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_issue(ws, pid, i1)}/")
    assert response.status_code == 200, response.text


def test_active_comments_unchanged(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    response = admin.get(f"{_issue(ws, pid, i1)}/comments/")
    assert response.status_code == 200, response.text
