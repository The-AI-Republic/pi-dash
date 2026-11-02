"""Core issue CRUD shapes (``app/urls/issue.py`` core paths).

Seeded world (see ``conftest.seed``): project IS with I1 (backlog/high,
label L1, assignee member, child I2, link, 2 assets, reaction, subscriber,
comment C1, relations), I2 (child of I1), I3 (completed), I4 (draft),
I5 (archived); project IS2 (guest_view_all off) with J1/J2.

Mutating tests create their own issues via the API and delete them, so
the seed stays pristine for the other files.
"""

LIST_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "created_at", "updated_at",
    "created_by", "updated_by", "is_draft", "archived_at", "deleted_at",
    "cycle_id", "link_count", "attachment_count", "sub_issues_count",
    "assignee_ids", "label_ids", "module_ids",
}

RETRIEVE_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id", "cycle_id",
    "type_id", "is_epic",
    "assigned_pod_id", "agent_executor", "module_ids", "label_ids",
    "assignee_ids", "sub_issues_count", "created_at", "updated_at",
    "created_by", "updated_by", "attachment_count", "link_count",
    "is_draft", "archived_at", "is_synced", "description_html",
    "is_subscribed", "agent_ticker", "agent_status", "relations_summary",
    "has_open_blockers",
}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_list_endpoint_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/list/?issues={i1}")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    row = body[0]
    assert set(row) == LIST_KEYS
    assert row["id"] == i1
    assert row["name"] == "I1 parent"
    assert row["priority"] == "high"
    assert row["sequence_id"] == 1
    assert row["link_count"] == 1
    assert row["sub_issues_count"] == 1
    assert row["label_ids"] == [seed["label1"]]
    assert row["assignee_ids"] == [seed["member"]]
    assert row["module_ids"] == []


def test_list_endpoint_requires_ids(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(f"{_base(ws, pid)}/issues/list/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "Issues are required"}


def test_crud_list_paginated_shape(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(f"{_base(ws, pid)}/issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == {
        "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
        "prev_cursor", "next_page_results", "prev_page_results", "count",
        "total_pages", "total_results", "extra_stats", "results",
    }
    # Draft I4 and archived I5 are excluded from the default list.
    assert body["total_count"] == 3
    names = {row["name"] for row in body["results"]}
    assert names == {"I1 parent", "I2 child", "I3 done"}
    for row in body["results"]:
        assert "state__group" in row


def test_crud_list_group_by_mismatch(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(f"{_base(ws, pid)}/issues/?group_by=state&sub_group_by=state")
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "Group by and sub group by cannot have same parameters"
    }


def test_create_minimal(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(f"{_base(ws, pid)}/issues/", json={"name": "Core probe"})
    assert resp.status_code == 201
    body = resp.json()
    assert set(body) == LIST_KEYS
    assert body["name"] == "Core probe"
    assert body["priority"] == "none"
    assert body["state_id"] == seed["state_backlog"]
    assert body["created_by"] == seed["admin"]
    assert body["assignee_ids"] == []
    assert body["label_ids"] == []
    assert isinstance(body["sequence_id"], int)
    issue_id = body["id"]
    assert admin.delete(f"{_base(ws, pid)}/issues/{issue_id}/").status_code == 204


def test_create_invalid_complexity(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/issues/",
        json={"name": "Bad score", "complexity_score": 11},
    )
    assert resp.status_code == 400
    assert "complexity_score" in resp.json()


def test_create_is_draft_500s(clients, seed):
    # Ported bug: creating with ``is_draft=true`` through the CRUD
    # endpoint 500s (``user_timezone_converter`` receives None because
    # the post-create values query filters the draft out). Drafts only
    # work through the workspace draft endpoints (a different URL
    # module, out of scope here).
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/issues/",
        json={"name": "Draft probe", "is_draft": True},
    )
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_draft_excluded_from_list_but_retrievable(clients, seed):
    admin = clients["admin"]
    ws, pid, i4 = seed["ws_slug"], seed["project"], seed["issue4"]
    listing = admin.get(f"{_base(ws, pid)}/issues/").json()
    assert i4 not in {row["id"] for row in listing["results"]}
    resp = admin.get(f"{_base(ws, pid)}/issues/{i4}/")
    assert resp.status_code == 200
    assert resp.json()["is_draft"] is True


def test_retrieve_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == RETRIEVE_KEYS
    assert body["name"] == "I1 parent"
    assert body["priority"] == "high"
    assert body["complexity_score"] == 0
    assert body["label_ids"] == [seed["label1"]]
    assert body["assignee_ids"] == [seed["member"]]
    assert body["sub_issues_count"] == 1
    assert body["link_count"] == 1
    # The attachments file runs earlier alphabetically and soft-deletes
    # one seeded asset, so only a lower bound is pinned here.
    assert body["attachment_count"] >= 1
    assert body["is_subscribed"] is False
    assert body["is_synced"] is False
    assert body["has_open_blockers"] is False
    assert body["relations_summary"]["blocked_by"] == [
        {"identifier": "IS-3", "state": "IS Done", "state_group": "completed"}
    ]
    assert body["relations_summary"]["blocking"] == []


def test_retrieve_missing_is_404(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/00000000-0000-0000-0000-000000000000/")
    assert resp.status_code == 404
    assert resp.json() == {"error": "The required object does not exist."}


def test_patch_then_put_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    created = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Roundtrip"}).json()
    issue_id = created["id"]
    try:
        resp = admin.patch(
            f"{_base(ws, pid)}/issues/{issue_id}/",
            json={"priority": "urgent"},
        )
        assert resp.status_code == 204
        assert resp.content == b""
        assert admin.get(
            f"{_base(ws, pid)}/issues/{issue_id}/").json()["priority"] == "urgent"
        resp = admin.put(
            f"{_base(ws, pid)}/issues/{issue_id}/",
            json={"name": "Roundtrip v2", "priority": "low"},
        )
        assert resp.status_code == 200
        assert resp.json()["name"] == "Roundtrip v2"
        assert resp.json()["priority"] == "low"
    finally:
        assert admin.delete(
            f"{_base(ws, pid)}/issues/{issue_id}/").status_code == 204
    assert admin.get(
        f"{_base(ws, pid)}/issues/{issue_id}/").status_code == 404


def test_patch_missing_is_404(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.patch(
        f"{_base(ws, pid)}/issues/00000000-0000-0000-0000-000000000000/",
        json={"priority": "low"},
    )
    assert resp.status_code == 404
    assert resp.json() == {"error": "Issue not found"}


def test_issues_detail_paginated(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(f"{_base(ws, pid)}/issues-detail/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["total_count"] == 3
    assert {row["name"] for row in body["results"]} == {
        "I1 parent", "I2 child", "I3 done"}


def test_v2_paginated_list(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(f"{_base(ws, pid)}/v2/issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == {
        "prev_cursor", "cursor", "next_cursor", "prev_page_results",
        "next_page_results", "page_count", "total_results", "total_pages",
        "results",
    }
    assert body["total_results"] == 3
    row = next(r for r in body["results"] if r["name"] == "I1 parent")
    assert set(row) == {
        "id", "name", "state_id", "state__group", "sort_order",
        "completed_at", "estimate_point", "priority", "start_date",
        "target_date", "sequence_id", "project_id", "parent_id", "cycle_id",
        "created_at", "updated_at", "created_by", "updated_by", "is_draft",
        "archived_at", "module_ids", "label_ids", "assignee_ids",
        "link_count", "attachment_count", "sub_issues_count",
    }
    assert "description_html" not in row
    resp = admin.get(f"{_base(ws, pid)}/v2/issues/?description=true")
    assert resp.status_code == 200
    row = next(
        r for r in resp.json()["results"] if r["name"] == "I1 parent")
    assert row["description_html"] == ""


def test_meta_endpoint(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/meta/")
    assert resp.status_code == 200
    assert resp.json() == {"sequence_id": 1, "project_identifier": "IS"}


def test_identifier_endpoint(clients, seed):
    admin = clients["admin"]
    ws, i1 = seed["ws_slug"], seed["issue1"]
    resp = admin.get(f"/api/workspaces/{ws}/work-items/IS-1/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["id"] == i1
    assert body["sequence_id"] == 1
    assert body["is_intake"] is False
    assert body["relations_summary"]["blocked_by"] == [
        {"identifier": "IS-3", "state": "IS Done", "state_group": "completed"}
    ]
    resp = admin.get(f"/api/workspaces/{ws}/work-items/IS-1/?lite=true")
    assert resp.status_code == 200
    assert set(resp.json()) == {
        "id", "sequence_id", "name", "description_html", "sort_order",
        "project_id", "created_at", "updated_at", "created_by",
        "updated_by", "is_draft", "is_epic", "is_intake", "is_synced",
        "archived_at",
    }
    resp = admin.get(f"/api/workspaces/{ws}/work-items/IS-xyz/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "Invalid issue identifier"}


def test_bulk_dates(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_base(ws, pid)}/issue-dates/",
        json={"updates": [{"id": i1, "start_date": "2026-01-01",
                           "target_date": "2026-02-01"}]},
    )
    assert resp.status_code == 200
    assert resp.json() == {"message": "Issues updated successfully"}
    body = admin.get(f"{_base(ws, pid)}/issues/{i1}/").json()
    assert body["start_date"] == "2026-01-01"
    assert body["target_date"] == "2026-02-01"
    resp = admin.post(
        f"{_base(ws, pid)}/issue-dates/",
        json={"updates": [{"id": i1, "start_date": "2026-03-01",
                           "target_date": "2026-02-01"}]},
    )
    assert resp.status_code == 400
    assert resp.json() == {"message": "Start date cannot exceed target date"}
    assert admin.patch(
        f"{_base(ws, pid)}/issues/{i1}/",
        json={"start_date": None, "target_date": None},
    ).status_code == 204


def test_user_properties(clients, seed):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/user-properties/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["user"] == seed["member"]
    assert body["project"] == pid
    assert set(body["display_properties"]) >= {"priority", "state", "labels"}
    resp = member.patch(
        f"{_base(ws, pid)}/user-properties/",
        json={"display_properties": {"key": "priority"}},
    )
    assert resp.status_code == 200
    assert resp.json()["display_properties"] == {"key": "priority"}


def test_deleted_issues_list(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(f"{_base(ws, pid)}/deleted-issues/")
    assert resp.status_code == 200
    assert i5 in resp.json()


def test_deleted_issues_list_order(clients, seed, db_conn):
    """``deleted-issues`` is ordered by ``Issue.Meta.ordering``
    (``-created_at``): the bare id array is newest-first."""
    import uuid
    from datetime import datetime, timedelta, timezone

    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project2"]  # IS2 has no tombstoned seed rows
    now = datetime.now(timezone.utc)
    inserted = []
    with db_conn.cursor() as cur:
        for i in range(3):
            iid = str(uuid.uuid4())
            created = now + timedelta(hours=i + 1)
            # Row 1 (i=1) is soft-deleted; the other two are archived.
            cur.execute(
                "INSERT INTO issues (created_at, updated_at, id, name,"
                " description_json, description_html, priority, sequence_id,"
                " sort_order, point, completed_at, is_draft, git_work_branch,"
                " workpad, complexity_score, created_by_id, project_id,"
                " state_id, workspace_id, archived_at, deleted_at)"
                " VALUES (%s,%s,%s,%s,'{}','<p></p>','none',%s,65535,NULL,"
                " NULL,false,'','',0,%s,%s,%s,%s,%s,%s)",
                (created, created, iid, f"tombstone {i}", 100 + i,
                 seed["admin"], seed["project2"], seed["state2_backlog"],
                 seed["ws"],
                 None if i == 1 else created.date(),
                 created if i == 1 else None),
            )
            inserted.append(iid)
    expected = list(reversed(inserted))  # newest-first
    try:
        resp = admin.get(f"{_base(ws, pid)}/deleted-issues/")
        assert resp.status_code == 200
        assert resp.json() == expected
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM issues WHERE id = ANY(%s::uuid[])", (inserted,))


def test_destroy_triage_state_issue(clients, seed, db_conn):
    """DELETE on an issue in a triage-group state soft-deletes (204).

    Django's destroy save reads ``self.state.group`` through
    ``State._base_manager`` (plain ``Manager``: no triage or soft-delete
    exclusion), so an intake issue deletes like any other
    (PIDASHCONV-1379). The row keeps no ``completed_at`` (triage is not
    ``completed``).
    """
    import uuid
    from datetime import datetime, timezone

    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    now = datetime.now(timezone.utc)
    state_id = str(uuid.uuid4())
    issue_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO states (created_at, updated_at, id, name, description,"
            " color, slug, project_id, workspace_id, sequence, \"group\","
            " \"default\", is_triage)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, state_id, "IS Triage", "", "#4E5355",
             "is-triage-destroy-probe",
             seed["project"], seed["ws"], 65000.0, "triage", False, True),
        )
        cur.execute(
            "INSERT INTO issues (created_at, updated_at, id, name,"
            " description_json, priority, sequence_id, created_by_id,"
            " project_id, state_id, workspace_id, description_html,"
            " sort_order, point, completed_at, is_draft, git_work_branch,"
            " workpad, complexity_score)"
            " VALUES (%s,%s,%s,%s,'{}','high',101,%s,%s,%s,%s,'',0.0,NULL,"
            " NULL,false,'','',0)",
            (now, now, issue_id, "Triage intake probe", seed["admin"],
             seed["project"], state_id, seed["ws"]),
        )
    try:
        resp = admin.delete(f"{_base(ws, pid)}/issues/{issue_id}/")
        assert resp.status_code == 204
        assert resp.content == b""
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT deleted_at, completed_at FROM issues WHERE id = %s",
                (issue_id,),
            )
            deleted_at, completed_at = cur.fetchone()
        assert deleted_at is not None
        assert completed_at is None
    finally:
        with db_conn.cursor() as cur:
            cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))
            cur.execute("DELETE FROM states WHERE id = %s", (state_id,))
