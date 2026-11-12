"""Contract tests: api-v1 module endpoints (7 routes, D-20).

Oracle for the Rust port. Mirrors test_cycles.py: exact statuses and shapes
over HTTP against the live server. Note the deliberate asymmetry with
cycles: the module-issue detail route only serves DELETE (an unrouted GET
falls through to 405), and archiving requires status completed/cancelled.
"""

import uuid

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

MODULE_KEYS = {
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
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
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

MODULE_ISSUE_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "module",
    "issue",
    "project",
    "workspace",
}

# POST / PATCH responses serialize the bare instance without the list/detail
# metric annotations.
CREATE_KEYS = MODULE_KEYS - {
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


class TestListModules:
    def test_list_default_shape(self, admin_client):
        r = admin_client.get(api.modules_url(SLUG, PROJ))
        assert r.status_code == 200
        body = r.json()
        assert PAGINATED_KEYS <= set(body.keys())
        assert body["total_count"] == 2
        by_name = {m["name"]: m for m in body["results"]}
        active = by_name["CT active module"]
        assert MODULE_KEYS <= set(active.keys())
        assert active["id"] == db.MODULE_ACTIVE_ID
        assert active["status"] == "planned"
        assert active["total_issues"] == 1
        assert active["unstarted_issues"] == 1
        assert active["members"] == []

    def test_list_excludes_archived(self, admin_client):
        r = admin_client.get(api.modules_url(SLUG, PROJ))
        names = {m["name"] for m in r.json()["results"]}
        assert "CT archived module" not in names


class TestCreateModule:
    def test_create_minimal(self, admin_client, db_conn):
        name = _name("CT module")
        r = admin_client.post(api.modules_url(SLUG, PROJ), json={"name": name})
        assert r.status_code == 201
        body = r.json()
        assert CREATE_KEYS <= set(body.keys())
        assert "total_issues" not in body
        assert body["name"] == name
        assert body["project"] == PROJ
        with db_conn.cursor() as cur:
            cur.execute("SELECT name FROM modules WHERE id = %s", [body["id"]])
            assert cur.fetchone()[0] == name

    def test_create_with_status_and_lead(self, admin_client):
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={"name": _name("CT module"), "status": "in-progress", "lead": db.ADMIN_ID},
        )
        assert r.status_code == 201
        assert r.json()["status"] == "in-progress"
        assert r.json()["lead"] == db.ADMIN_ID

    def test_create_invalid(self, admin_client):
        assert admin_client.post(api.modules_url(SLUG, PROJ), json={}).status_code == 400
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={
                "name": _name("CT module"),
                "start_date": "2026-10-08",
                "target_date": "2026-10-01",
            },
        )
        assert r.status_code == 400

    def test_create_duplicate_name(self, admin_client):
        name = _name("CT module")
        assert admin_client.post(api.modules_url(SLUG, PROJ), json={"name": name}).status_code == 201
        r = admin_client.post(api.modules_url(SLUG, PROJ), json={"name": name})
        assert r.status_code == 400
        assert r.json()["code"] == "MODULE_NAME_ALREADY_EXISTS"

    def test_create_duplicate_external_id(self, admin_client):
        payload = {
            "name": _name("CT module"),
            "external_id": "ct-mod-ext-1",
            "external_source": "github",
        }
        assert admin_client.post(api.modules_url(SLUG, PROJ), json=payload).status_code == 201
        payload["name"] = _name("CT module")
        r = admin_client.post(api.modules_url(SLUG, PROJ), json=payload)
        assert r.status_code == 409
        assert "same external id" in r.json()["error"]

    def test_create_forbidden_for_guest(self, guest_client):
        # Coverage floor: denied-permission case.
        r = guest_client.post(api.modules_url(SLUG, PROJ), json={"name": _name("CT")})
        assert r.status_code == 403

    def test_create_isolated_between_tenants(self, outsider_client):
        # Coverage floor: tenant isolation.
        r = outsider_client.get(api.modules_url(SLUG, PROJ))
        assert r.status_code == 403
        r = outsider_client.post(api.modules_url(SLUG, PROJ), json={"name": _name("CT")})
        assert r.status_code == 403


class TestModuleDetail:
    def test_get_shape(self, admin_client):
        r = admin_client.get(api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID))
        assert r.status_code == 200
        body = r.json()
        assert MODULE_KEYS <= set(body.keys())
        assert body["total_issues"] == 1
        assert body["backlog_issues"] == 0

    def test_get_not_found(self, admin_client):
        r = admin_client.get(api.module_detail_url(SLUG, PROJ, UNKNOWN_ID))
        assert r.status_code == 404

    def test_patch_rename(self, admin_client):
        name = _name("CT renamed")
        r = admin_client.patch(
            api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID),
            json={"name": name},
        )
        assert r.status_code == 200
        assert r.json()["name"] == name

    def test_patch_archived_module_rejected(self, admin_client):
        r = admin_client.patch(
            api.module_detail_url(SLUG, PROJ, db.MODULE_ARCHIVED_ID),
            json={"name": _name("CT")},
        )
        assert r.status_code == 400

    def test_patch_external_id_conflict(self, admin_client):
        admin_client.patch(
            api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID),
            json={"external_id": "ct-mod-ext-9", "external_source": "github"},
        )
        r = admin_client.patch(
            api.module_detail_url(SLUG, PROJ, db.MODULE_COMPLETED_ID),
            json={"external_id": "ct-mod-ext-9", "external_source": "github"},
        )
        assert r.status_code == 409
        assert "same external id" in r.json()["error"]

    def test_delete(self, admin_client, db_conn):
        r = admin_client.post(api.modules_url(SLUG, PROJ), json={"name": _name("CT")})
        mid = r.json()["id"]
        r = admin_client.delete(api.module_detail_url(SLUG, PROJ, mid))
        assert r.status_code == 204
        # Deletes are soft: the row stays with deleted_at set.
        row = db.fetch_module(db_conn, mid)
        assert row is not None and row[2] is not None

    def test_delete_forbidden_for_non_admin_non_creator(self, member_client):
        # Coverage floor: denied-permission case.
        r = member_client.delete(api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID))
        assert r.status_code == 403
        assert "Only admin or creator" in r.json()["error"]


class TestModuleIssues:
    def test_list_shape(self, admin_client):
        r = admin_client.get(api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID))
        assert r.status_code == 200
        body = r.json()
        assert PAGINATED_KEYS <= set(body.keys())
        assert body["total_count"] == 1
        assert ISSUE_KEYS <= set(body["results"][0].keys())
        assert body["results"][0]["id"] == db.ISSUE_UNSTARTED_ID

    def test_add_and_remove_issue(self, admin_client, db_conn):
        r = admin_client.post(
            api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID),
            json={"issues": [db.ISSUE_STARTED_ID]},
        )
        assert r.status_code == 200
        assert MODULE_ISSUE_KEYS <= set(r.json()[0].keys())

        r = admin_client.delete(
            api.module_issue_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID, db.ISSUE_STARTED_ID)
        )
        assert r.status_code == 204
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT count(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
                " AND deleted_at IS NULL",
                [db.MODULE_ACTIVE_ID, db.ISSUE_STARTED_ID],
            )
            assert cur.fetchone()[0] == 0

    def test_add_requires_issues(self, admin_client):
        r = admin_client.post(
            api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID), json={"issues": []}
        )
        assert r.status_code == 400
        assert r.json()["error"] == "Issues are required"

    def test_add_issues_missing_module_404_beats_uuid_400(self, admin_client):
        # PIDASHCONV-1194: the module 404 wins over the invalid-UUID 400.
        # Django runs `if not len(issues)` (a length/shape check only), then
        # Module.objects.get(...) -> 404, and only afterwards does
        # pk__in validate the UUIDs. A non-empty string/dict passes the gate
        # and iterates into invalid candidates, but the missing module must
        # short-circuit to 404 first.
        url = api.module_issues_url(SLUG, PROJ, UNKNOWN_ID)
        for body in (
            {"issues": ["not-a-uuid"]},
            {"issues": "abc"},
            {"issues": {"x": 1}},
        ):
            r = admin_client.post(url, json=body)
            assert r.status_code == 404, body
            assert r.json() == {"error": "The requested resource does not exist."}

    def test_add_invalid_uuid_existing_module_400(self, admin_client):
        # The arm that survives the reorder: with the module present, the
        # invalid UUID still answers the DRF ValidationError 400.
        r = admin_client.post(
            api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID),
            json={"issues": ["not-a-uuid"]},
        )
        assert r.status_code == 400
        assert r.json() == {"error": "Please provide valid detail"}

    def test_detail_get_is_not_routed(self, admin_client):
        # The view defines get() but urls/module.py only routes DELETE on the
        # detail path; an unrouted GET falls through to 405. Documented so
        # the port does not invent the endpoint.
        r = admin_client.get(
            api.module_issue_detail_url(
                SLUG, PROJ, db.MODULE_ACTIVE_ID, db.ISSUE_UNSTARTED_ID
            )
        )
        assert r.status_code == 405


class TestArchiveModules:
    def test_archive_completed_and_unarchive(self, admin_client):
        r = admin_client.post(api.module_archive_url(SLUG, PROJ, db.MODULE_COMPLETED_ID))
        assert r.status_code == 204

        r = admin_client.get(api.archived_modules_url(SLUG, PROJ))
        assert r.status_code == 200
        assert PAGINATED_KEYS <= set(r.json().keys())
        names = {m["name"] for m in r.json()["results"]}
        assert {"CT completed module", "CT archived module"} <= names
        assert MODULE_KEYS <= set(r.json()["results"][0].keys())

        r = admin_client.delete(
            api.module_unarchive_url(SLUG, PROJ, db.MODULE_COMPLETED_ID)
        )
        assert r.status_code == 204
        r = admin_client.get(api.archived_modules_url(SLUG, PROJ))
        assert {m["name"] for m in r.json()["results"]} == {"CT archived module"}

    def test_archive_non_completed_rejected(self, admin_client):
        r = admin_client.post(api.module_archive_url(SLUG, PROJ, db.MODULE_ACTIVE_ID))
        assert r.status_code == 400
        assert "Only completed or cancelled" in r.json()["error"]

    def test_unauthenticated_is_rejected(self, anon_client):
        assert anon_client.get(api.modules_url(SLUG, PROJ)).status_code == 401


DATE_FORMAT_ERROR = {
    "start_date": [
        "Date has wrong format. Use one of these formats instead: YYYY-MM-DD."
    ]
}


class TestModuleDateFormats:
    """PIDASHCONV-512: Django's parse_date is fromisoformat-first, so ISO
    week dates and basic YYYYMMDD validate and store as Gregorian dates;
    ordinal dates do not."""

    def test_create_week_date_extended(self, admin_client, db_conn):
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={
                "name": _name("CT module"),
                "start_date": "2024-W05-6",
                "target_date": "2024-W06-1",
            },
        )
        assert r.status_code == 201
        assert r.json()["start_date"] == "2024-02-03"
        assert r.json()["target_date"] == "2024-02-05"
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT start_date, target_date FROM modules WHERE id = %s",
                [r.json()["id"]],
            )
            row = cur.fetchone()
            assert (str(row[0]), str(row[1])) == ("2024-02-03", "2024-02-05")

    def test_create_basic_format(self, admin_client):
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={
                "name": _name("CT module"),
                "start_date": "20240105",
                "target_date": "20240203",
            },
        )
        assert r.status_code == 201
        assert r.json()["start_date"] == "2024-01-05"
        assert r.json()["target_date"] == "2024-02-03"

    def test_create_week_date_dayless_and_basic(self, admin_client):
        # A missing day means Monday, extended and basic alike.
        for raw, expected in (("2024-W05", "2024-01-29"), ("2024W056", "2024-02-03")):
            r = admin_client.post(
                api.modules_url(SLUG, PROJ),
                json={"name": _name("CT module"), "start_date": raw},
            )
            assert r.status_code == 201
            assert r.json()["start_date"] == expected

    def test_patch_week_date(self, admin_client):
        r = admin_client.patch(
            api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID),
            json={"start_date": "20240105", "target_date": "2024-W05-6"},
        )
        assert r.status_code == 200
        assert r.json()["start_date"] == "2024-01-05"
        assert r.json()["target_date"] == "2024-02-03"

    def test_create_rejects_ordinal_and_bad_week(self, admin_client):
        for raw in ("2024-036", "2021-W53-1"):
            r = admin_client.post(
                api.modules_url(SLUG, PROJ),
                json={"name": _name("CT module"), "start_date": raw},
            )
            assert r.status_code == 400
            assert r.json() == DATE_FORMAT_ERROR


class TestModuleMembersSoftDeleted:
    """PIDASHCONV-1418: v1 `members` renders soft-deleted ModuleMember rows.

    Django's M2M `instance.members.all()` joins the through table with no
    soft-delete filter (serializers/module.py:203-206), while PATCH
    soft-deletes the old rows then bulk-creates the new ones (:149). So
    after any member-list change the payload keeps the dead ids too,
    including duplicates. Sets/counts only: seed users share one
    `now()` created_at, so `-created_at` order ties."""

    def test_members_after_patch_include_soft_deleted(self, admin_client):
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={"name": _name("CT members"), "members": [db.ADMIN_ID]},
        )
        assert r.status_code == 201
        mid = r.json()["id"]
        r = admin_client.patch(
            api.module_detail_url(SLUG, PROJ, mid),
            json={"members": [db.MEMBER_ID]},
        )
        assert r.status_code == 200
        r = admin_client.get(api.module_detail_url(SLUG, PROJ, mid))
        assert r.status_code == 200
        assert set(r.json()["members"]) == {db.ADMIN_ID, db.MEMBER_ID}
        r = admin_client.get(api.modules_url(SLUG, PROJ))
        assert r.status_code == 200
        found = [m for m in r.json()["results"] if m["id"] == mid]
        assert len(found) == 1
        assert set(found[0]["members"]) == {db.ADMIN_ID, db.MEMBER_ID}

    def test_members_patch_same_member_duplicates(self, admin_client):
        r = admin_client.post(
            api.modules_url(SLUG, PROJ),
            json={"name": _name("CT members"), "members": [db.ADMIN_ID]},
        )
        assert r.status_code == 201
        mid = r.json()["id"]
        url = api.module_detail_url(SLUG, PROJ, mid)
        r = admin_client.patch(url, json={"members": [db.ADMIN_ID]})
        assert r.status_code == 200
        r = admin_client.get(url)
        assert r.json()["members"].count(db.ADMIN_ID) == 2
        r = admin_client.patch(url, json={"members": [db.ADMIN_ID]})
        assert r.status_code == 200
        r = admin_client.get(url)
        members = r.json()["members"]
        assert len(members) == 3
        assert members.count(db.ADMIN_ID) == 3
