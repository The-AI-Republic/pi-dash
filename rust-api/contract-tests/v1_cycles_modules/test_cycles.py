"""Contract tests: api-v1 cycle endpoints (8 routes, D-20).

Oracle for the Rust port. Every test drives the live server over HTTP and
asserts the exact status codes and response shapes Django produces today —
including two known-upstream quirks that the port must reproduce:
``cycle_view=current`` returns a bare list (not a paginated envelope), and
archiving a draft cycle (``end_date`` null) raises a 500.
"""

import uuid

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

CYCLE_KEYS = {
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
}

PAGINATED_KEYS = {
    "grouped_by",
    "sub_grouped_by",
    "total_count",
    "next_cursor",
    "prev_cursor",
    "next_page_results",
    "prev_page_results",
    "count",
    "total_pages",
    "total_results",
    "extra_stats",
    "results",
}

ISSUE_KEYS = {
    "id",
    "name",
    "project",
    "workspace",
    "state",
    "priority",
    "sequence_id",
    "assignees",
    "labels",
    "is_draft",
    "archived_at",
    "created_at",
    "updated_at",
}

CYCLE_ISSUE_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "cycle",
    "issue",
    "project",
    "workspace",
}

# POST / PATCH responses serialize the bare instance without the list/detail
# metric annotations (the view re-serializes without get_queryset()).
CREATE_KEYS = CYCLE_KEYS - {
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
}

UNKNOWN_ID = "99999999-9999-4999-8999-999999999999"


def _name(prefix):
    return f"{prefix} {uuid.uuid4().hex[:8]}"


class TestListCycles:
    def test_list_default_shape(self, admin_client):
        r = admin_client.get(api.cycles_url(SLUG, PROJ))
        assert r.status_code == 200
        body = r.json()
        assert PAGINATED_KEYS <= set(body.keys())
        assert body["total_count"] == 3
        by_name = {c["name"]: c for c in body["results"]}
        active = by_name["CT active cycle"]
        assert CYCLE_KEYS <= set(active.keys())
        assert active["id"] == db.CYCLE_ACTIVE_ID
        assert active["total_issues"] == 2
        assert active["completed_issues"] == 1
        assert active["started_issues"] == 1
        assert active["archived_at"] is None

    def test_list_excludes_archived(self, admin_client):
        r = admin_client.get(api.cycles_url(SLUG, PROJ))
        names = {c["name"] for c in r.json()["results"]}
        assert "CT archived cycle" not in names

    def test_list_current_view_returns_bare_list(self, admin_client):
        # Quirk: every other view returns the paginated envelope, but
        # cycle_view=current serializes the queryset directly (see
        # CycleListCreateAPIEndpoint.get). The port reproduces this.
        r = admin_client.get(api.cycles_url(SLUG, PROJ), params={"cycle_view": "current"})
        assert r.status_code == 200
        body = r.json()
        assert isinstance(body, list)
        assert [c["name"] for c in body] == ["CT active cycle"]

    def test_list_upcoming_completed_draft_views(self, admin_client):
        for view, expected in [
            ("upcoming", []),
            ("completed", ["CT completed cycle"]),
            ("draft", ["CT draft cycle"]),
        ]:
            r = admin_client.get(api.cycles_url(SLUG, PROJ), params={"cycle_view": view})
            assert r.status_code == 200
            body = r.json()
            assert PAGINATED_KEYS <= set(body.keys())
            assert [c["name"] for c in body["results"]] == expected

    def test_list_member_can_read(self, member_client):
        r = member_client.get(api.cycles_url(SLUG, PROJ))
        assert r.status_code == 200
        assert r.json()["total_count"] == 3


class TestCreateCycle:
    def test_create_minimal_draft(self, admin_client, db_conn):
        r = admin_client.post(api.cycles_url(SLUG, PROJ), json={"name": _name("CT")})
        assert r.status_code == 201
        body = r.json()
        assert CREATE_KEYS <= set(body.keys())
        assert "total_issues" not in body
        assert body["start_date"] is None and body["end_date"] is None
        assert body["owned_by"] == db.ADMIN_ID
        with db_conn.cursor() as cur:
            cur.execute("SELECT name, project_id FROM cycles WHERE id = %s", [body["id"]])
            row = cur.fetchone()
        assert row is not None and str(row[1]) == PROJ

    def test_create_with_dates(self, admin_client):
        r = admin_client.post(
            api.cycles_url(SLUG, PROJ),
            json={
                "name": _name("CT"),
                "start_date": "2026-10-01T00:00:00Z",
                "end_date": "2026-10-08T00:00:00Z",
            },
        )
        assert r.status_code == 201
        assert r.json()["start_date"].startswith("2026-10-01")
        assert r.json()["end_date"].startswith("2026-10-08")

    def test_create_invalid(self, admin_client):
        assert admin_client.post(api.cycles_url(SLUG, PROJ), json={}).status_code == 400
        r = admin_client.post(
            api.cycles_url(SLUG, PROJ),
            json={"name": _name("CT"), "start_date": "2026-10-01T00:00:00Z"},
        )
        assert r.status_code == 400
        assert "Both start date and end date" in r.json()["error"]

    def test_create_duplicate_external_id(self, admin_client):
        payload = {
            "name": _name("CT"),
            "external_id": "ct-ext-1",
            "external_source": "github",
        }
        assert admin_client.post(api.cycles_url(SLUG, PROJ), json=payload).status_code == 201
        payload["name"] = _name("CT")
        r = admin_client.post(api.cycles_url(SLUG, PROJ), json=payload)
        assert r.status_code == 409
        assert "same external id" in r.json()["error"]

    def test_create_forbidden_for_guest(self, guest_client):
        # Coverage floor: denied-permission case (GUEST=5 may read, not write).
        r = guest_client.post(api.cycles_url(SLUG, PROJ), json={"name": _name("CT")})
        assert r.status_code == 403

    def test_create_isolated_between_tenants(self, outsider_client):
        # Coverage floor: tenant isolation — a valid token from workspace B
        # sees nothing of workspace A's project.
        r = outsider_client.get(api.cycles_url(SLUG, PROJ))
        assert r.status_code == 403
        r = outsider_client.post(api.cycles_url(SLUG, PROJ), json={"name": _name("CT")})
        assert r.status_code == 403


class TestCycleDetail:
    def test_get_shape(self, admin_client):
        r = admin_client.get(api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID))
        assert r.status_code == 200
        body = r.json()
        assert CYCLE_KEYS <= set(body.keys())
        assert body["total_issues"] == 2
        assert body["backlog_issues"] == 0

    def test_get_not_found(self, admin_client):
        r = admin_client.get(api.cycle_detail_url(SLUG, PROJ, UNKNOWN_ID))
        assert r.status_code == 404

    def test_patch_rename(self, admin_client):
        name = _name("CT renamed")
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID),
            json={"name": name},
        )
        assert r.status_code == 200
        assert r.json()["name"] == name

    def test_patch_completed_cycle_rejected(self, admin_client):
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"name": _name("CT")},
        )
        assert r.status_code == 400
        assert "already been completed" in r.json()["error"]

    def test_patch_completed_cycle_sort_order_silently_dropped(self, admin_client):
        # Ported upstream bug: the view narrows the payload to sort_order for
        # completed cycles, but the serializer is built from request.data and
        # CycleUpdateSerializer has no sort_order field — so the edit is a
        # 200 no-op. The Rust port reproduces this until upstream fixes it.
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"sort_order": 12.5},
        )
        assert r.status_code == 200
        assert r.json()["sort_order"] == 65535.0

    def test_patch_archived_cycle_rejected(self, admin_client):
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ARCHIVED_ID),
            json={"name": _name("CT")},
        )
        assert r.status_code == 400

    def test_patch_external_id_conflict(self, admin_client):
        admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID),
            json={"external_id": "ct-ext-9", "external_source": "github"},
        )
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"external_id": "ct-ext-9", "external_source": "github"},
        )
        # Completed cycles reject every edit except sort_order before the
        # conflict check runs; use the draft cycle as the conflict target.
        assert r.status_code in (400, 409)
        r = admin_client.patch(
            api.cycle_detail_url(SLUG, PROJ, db.CYCLE_DRAFT_ID),
            json={"external_id": "ct-ext-9", "external_source": "github"},
        )
        assert r.status_code == 409
        assert "same external id" in r.json()["error"]

    def test_delete(self, admin_client, db_conn):
        r = admin_client.post(api.cycles_url(SLUG, PROJ), json={"name": _name("CT")})
        cid = r.json()["id"]
        r = admin_client.delete(api.cycle_detail_url(SLUG, PROJ, cid))
        assert r.status_code == 204
        # Deletes are soft: the row stays with deleted_at set.
        row = db.fetch_cycle(db_conn, cid)
        assert row is not None and row[2] is not None

    def test_delete_forbidden_for_non_admin_non_creator(self, member_client):
        # Coverage floor: denied-permission case — a project MEMBER who
        # neither owns the cycle nor administrates the project cannot delete.
        r = member_client.delete(api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID))
        assert r.status_code == 403
        assert "Only admin or creator" in r.json()["error"]


class TestCycleIssues:
    def test_list_shape(self, admin_client):
        r = admin_client.get(api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID))
        assert r.status_code == 200
        body = r.json()
        assert PAGINATED_KEYS <= set(body.keys())
        assert body["total_count"] == 2
        first = body["results"][0]
        assert ISSUE_KEYS <= set(first.keys())

    def test_add_and_remove_issue(self, admin_client, db_conn):
        r = admin_client.post(
            api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID),
            json={"issues": [db.ISSUE_BACKLOG_ID]},
        )
        assert r.status_code == 200
        assert CYCLE_ISSUE_KEYS <= set(r.json()[0].keys())

        r = admin_client.get(
            api.cycle_issue_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID, db.ISSUE_BACKLOG_ID)
        )
        assert r.status_code == 200
        assert r.json()["issue"] == db.ISSUE_BACKLOG_ID

        r = admin_client.delete(
            api.cycle_issue_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID, db.ISSUE_BACKLOG_ID)
        )
        assert r.status_code == 204
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT count(*) FROM cycle_issues WHERE cycle_id = %s AND issue_id = %s"
                " AND deleted_at IS NULL",
                [db.CYCLE_ACTIVE_ID, db.ISSUE_BACKLOG_ID],
            )
            assert cur.fetchone()[0] == 0

    def test_add_requires_issues(self, admin_client):
        r = admin_client.post(
            api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID), json={"issues": []}
        )
        assert r.status_code == 400
        assert r.json()["code"] == "MISSING_WORK_ITEMS"

    def test_add_to_completed_cycle_rejected(self, admin_client):
        r = admin_client.post(
            api.cycle_issues_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"issues": [db.ISSUE_BACKLOG_ID]},
        )
        assert r.status_code == 400
        assert r.json()["code"] == "CYCLE_COMPLETED"


class TestTransferCycleIssues:
    def test_transfer_missing_target(self, admin_client):
        r = admin_client.post(
            api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID), json={}
        )
        assert r.status_code == 400
        assert r.json()["error"] == "New Cycle Id is required"

    def test_transfer_from_incomplete_cycle_rejected(self, admin_client):
        r = admin_client.post(
            api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID),
            json={"new_cycle_id": db.CYCLE_COMPLETED_ID},
        )
        assert r.status_code == 400
        assert r.json()["error"] == "The old cycle is not completed yet"

    def test_transfer_success_moves_incomplete_issues(self, admin_client, db_conn):
        # The add endpoint refuses completed cycles, so link an open-state
        # issue to the completed cycle directly, then transfer.
        with db_conn.cursor() as cur:
            cur.execute(
                "INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id,"
                " issue_id, project_id, workspace_id) VALUES (gen_random_uuid(),"
                " now(), now(), %s, %s, %s, %s)",
                [db.CYCLE_COMPLETED_ID, db.ISSUE_BACKLOG_ID, PROJ, db.WS_A_ID],
            )
            db_conn.commit()
        r = admin_client.post(
            api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"new_cycle_id": db.CYCLE_ACTIVE_ID},
        )
        assert r.status_code == 200
        assert r.json() == {"message": "Success"}
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT cycle_id FROM cycle_issues WHERE issue_id = %s AND deleted_at IS NULL",
                [db.ISSUE_BACKLOG_ID],
            )
            assert str(cur.fetchone()[0]) == db.CYCLE_ACTIVE_ID
            cur.execute(
                "SELECT progress_snapshot FROM cycles WHERE id = %s",
                [db.CYCLE_COMPLETED_ID],
            )
            snapshot = cur.fetchone()[0]
            assert snapshot["total_issues"] == 1
            assert "distribution" in snapshot


class TestArchiveCycles:
    def test_archive_completed_and_unarchive(self, admin_client):
        r = admin_client.post(api.cycle_archive_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID))
        assert r.status_code == 204

        r = admin_client.get(api.archived_cycles_url(SLUG, PROJ))
        assert r.status_code == 200
        assert PAGINATED_KEYS <= set(r.json().keys())
        names = {c["name"] for c in r.json()["results"]}
        assert {"CT completed cycle", "CT archived cycle"} <= names
        assert CYCLE_KEYS <= set(r.json()["results"][0].keys())

        r = admin_client.delete(
            api.cycle_unarchive_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID)
        )
        assert r.status_code == 204
        r = admin_client.get(api.archived_cycles_url(SLUG, PROJ))
        assert {c["name"] for c in r.json()["results"]} == {"CT archived cycle"}

    def test_archive_draft_cycle_errors(self, admin_client):
        # Ported upstream bug: archiving a cycle with null end_date compares
        # None >= now, raising TypeError -> generic 500 envelope. The Rust
        # port reproduces the 500 until upstream fixes it (tracked in the PR).
        r = admin_client.post(api.cycle_archive_url(SLUG, PROJ, db.CYCLE_DRAFT_ID))
        assert r.status_code == 500
        assert "error" in r.json()

    def test_unauthenticated_is_rejected(self, anon_client):
        assert anon_client.get(api.cycles_url(SLUG, PROJ)).status_code == 401
