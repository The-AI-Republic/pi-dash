"""Modules CRUD (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/modules/
  GET/PUT/PATCH/DELETE /api/workspaces/<slug>/projects/<project_id>/modules/<pk>/

PUT has no custom action: it falls back to DRF's default update over
ModuleWriteSerializer, so its key set differs from the annotated
create/PATCH shapes. Pinned here on purpose.
"""

from datetime import datetime, timedelta, timezone

from .conftest import (
    MODULE_DETAIL_KEYS as DETAIL_KEYS,
    MODULE_ROW_KEYS,
    module_url,
    modules_url,
    session_headers,
)

from _harness.seed import estimate, estimate_point

# PUT renders the bare ModuleWriteSerializer instance (fields="__all__"
# plus member_ids), exposing created_by/updated_by/project/workspace.
WRITE_MODULE_KEYS = {
    "id", "lead_id", "created_at", "updated_at", "deleted_at",
    "name", "description", "description_text", "description_html",
    "start_date", "target_date", "status", "view_props", "sort_order",
    "external_source", "external_id", "archived_at", "logo_props",
    "created_by", "updated_by", "project", "workspace", "lead",
    "members", "member_ids",
}


def test_create_shape(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a), headers=auth_a,
        json={"name": "Alpha module"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body) == MODULE_ROW_KEYS, sorted(set(body) ^ MODULE_ROW_KEYS)
    assert body["name"] == "Alpha module"
    assert body["status"] == "planned"
    assert body["project_id"] == project_a
    assert body["workspace_id"] == tenant_a["workspace"]["id"]
    assert body["is_favorite"] is False
    assert body["total_issues"] == 0
    assert body["member_ids"] == []


def test_create_duplicate_name_rejected(api, tenant_a, project_a, auth_a, seed):
    seed.module(tenant_a["workspace"]["id"], project_a, name="Taken")
    r = api.post(
        modules_url(tenant_a, project_a), headers=auth_a,
        json={"name": "Taken"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Module with this name already exists"}


def test_create_start_after_target_rejected(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a), headers=auth_a,
        json={"name": "Bad dates", "start_date": "2026-10-01", "target_date": "2026-09-01"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"non_field_errors": ["Start date cannot exceed target date"]}


def test_list_shape(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(tenant_a["workspace"]["id"], project_a, name="Listed")
    r = api.get(modules_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list)
    entry = next(m for m in body if m["id"] == mid)
    assert set(entry) == MODULE_ROW_KEYS, sorted(set(entry) ^ MODULE_ROW_KEYS)
    assert entry["name"] == "Listed"


def test_retrieve_shape(api, seed, tenant_a, project_a, auth_a, module_a):
    r = api.get(module_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == DETAIL_KEYS, sorted(set(body) ^ DETAIL_KEYS)
    assert body["id"] == module_a
    assert body["link_module"] == []
    assert body["sub_issues"] == 0
    assert body["estimate_distribution"] == {}
    assert set(body["distribution"]) == {"assignees", "labels", "completion_chart"}


def test_retrieve_missing(api, tenant_a, project_a, auth_a):
    r = api.get(
        module_url(tenant_a, project_a, "00000000-0000-0000-0000-000000000000"),
        headers=auth_a,
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "Module not found"}


def test_put_shape(api, seed, tenant_a, project_a, auth_a, module_a):
    """PUT uses DRF's default update (no custom action), so the response
    is the write serializer, not the annotated row."""
    r = api.put(
        module_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"name": "Renamed", "description": "via put"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == WRITE_MODULE_KEYS, sorted(set(body) ^ WRITE_MODULE_KEYS)
    assert body["name"] == "Renamed"
    assert body["description"] == "via put"
    assert body["project"] == project_a


def test_partial_update(api, tenant_a, project_a, auth_a, module_a):
    r = api.patch(
        module_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"name": "Patched"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == MODULE_ROW_KEYS, sorted(set(body) ^ MODULE_ROW_KEYS)
    assert body["name"] == "Patched"


def test_destroy(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(tenant_a["workspace"]["id"], project_a, name="Doomed")
    r = api.delete(module_url(tenant_a, project_a, mid), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(module_url(tenant_a, project_a, mid), headers=auth_a)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "Module not found"}


def test_destroy_with_two_deleted_bridges_same_pair(api, seed, tenant_a, project_a,
                                                     auth_a, module_a, issue_a):
    """PIDASHCONV-875: attach → detach → re-attach → detach leaves two
    soft-deleted bridges for one (issue, module) pair; the module DELETE
    must still 204 (Django only re-stamps live bridges, so the re-stamp
    never collides on unique (issue_id, module_id, deleted_at))."""
    attach_url = f"{modules_url(tenant_a, project_a)}{module_a}/issues/"
    detach_url = f"{attach_url}{issue_a}/"
    for _ in range(2):
        r = api.post(attach_url, headers=auth_a, json={"issues": [issue_a]})
        assert r.status_code == 201, r.text
        r = api.delete(detach_url, headers=auth_a)
        assert r.status_code == 204, r.text
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
            " AND deleted_at IS NOT NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 2
    r = api.delete(module_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(module_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "Module not found"}


def test_destroy_missing(api, tenant_a, project_a, auth_a):
    r = api.delete(
        module_url(tenant_a, project_a, "00000000-0000-0000-0000-000000000000"),
        headers=auth_a,
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "The required object does not exist."}


def test_destroy_denied_for_member_non_creator(api, seed, tenant_a, project_a, auth_a, module_a):
    """Destroy needs ADMIN + creator; a project member who did not create
    the module is rejected. This is the suite's denied-permission case
    for the destroy action."""
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=15)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=15)
    import os
    other_headers = session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.delete(module_url(tenant_a, project_a, module_a), headers=other_headers)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_requires_auth(api, tenant_a, project_a):
    r = api.get(modules_url(tenant_a, project_a))
    assert r.status_code == 401, r.text
    assert r.json() == {"detail": "Authentication credentials were not provided."}


def test_create_sort_order_without_siblings(api, tenant_a, project_a, auth_a):
    """Module.save only clobbers sort_order when siblings exist: the first
    module in a project keeps its validated input (default 65535.0 when
    absent); later creates take MIN(sibling) - 10000 regardless of input."""
    r = api.post(
        modules_url(tenant_a, project_a), headers=auth_a,
        json={"name": "First", "sort_order": 42},
    )
    assert r.status_code == 201, r.text
    assert r.json()["sort_order"] == 42.0
    r = api.post(
        modules_url(tenant_a, project_a), headers=auth_a,
        json={"name": "Second", "sort_order": 7},
    )
    assert r.status_code == 201, r.text
    assert r.json()["sort_order"] == 42.0 - 10000


# `ModuleViewSet.list` (`base.py:353-393`) branches on `fields` =
# `[f for f in request.GET.get("fields", "").split(",") if f]`
# (`base.py:180-183`). Non-empty: `ModuleSerializer(queryset, many=True,
# fields=self.fields).data` — the projection is discarded
# (`serializers/base.py:16-24`) but the row is the full
# `ModuleSerializer.Meta.fields` set; otherwise the annotated `.values()`
# row. The two shapes differ by the extra `archived_at` and the key order,
# so both are pinned here.
MODULE_SERIALIZER_ORDER = [
    "id", "workspace_id", "project_id", "name", "description",
    "description_text", "description_html", "start_date", "target_date",
    "status", "lead_id", "member_ids", "view_props", "sort_order",
    "external_source", "external_id", "logo_props",
    "total_estimate_points", "completed_estimate_points", "is_favorite",
    "total_issues", "cancelled_issues", "completed_issues",
    "started_issues", "unstarted_issues", "backlog_issues",
    "created_at", "updated_at", "archived_at",
]

MODULE_VALUES_ORDER = [
    "id", "workspace_id", "project_id", "name", "description",
    "description_text", "description_html", "start_date", "target_date",
    "status", "lead_id", "view_props", "sort_order", "external_source",
    "external_id", "logo_props", "created_at", "updated_at",
    "is_favorite", "completed_issues", "cancelled_issues", "started_issues",
    "unstarted_issues", "backlog_issues", "total_issues",
    "completed_estimate_points", "total_estimate_points", "member_ids",
]


def test_list_fields_branch_shape(api, seed, tenant_a, project_a, auth_a):
    """`?fields=name` takes Django's ModuleSerializer branch: 29 keys in
    Meta.fields order, `archived_at` present as null, and the same values
    as the annotated row for the 28 shared keys (the projection is not
    applied)."""
    mid = seed.module(tenant_a["workspace"]["id"], project_a, name="Projected")
    r = api.get(modules_url(tenant_a, project_a) + "?fields=name", headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list)
    entry = next(m for m in body if m["id"] == mid)
    assert list(entry) == MODULE_SERIALIZER_ORDER, list(entry)
    assert entry["archived_at"] is None
    assert entry["name"] == "Projected"

    plain = api.get(modules_url(tenant_a, project_a), headers=auth_a)
    assert plain.status_code == 200, plain.text
    plain_entry = next(m for m in plain.json() if m["id"] == mid)
    assert {k: entry[k] for k in plain_entry} == plain_entry


def test_distribution_keeps_soft_deleted_bridges(api, seed, db, tenant_a,
                                               project_a, auth_a):
    """Removed assignees/labels stay in the distributions (PIDASHCONV-1424).

    Django groups through the ``Issue.assignees`` / ``Issue.labels`` M2M
    fields (``app/views/module/base.py``), whose joins never apply the
    soft-delete manager — so a reassigned user and a swapped label keep
    their rows, even when the label row itself is soft-deleted
    (PB-0102 class, already ported for cycles). The Rust port filtered
    the bridges (and the label row) out of ``distribution``,
    ``estimate_distribution`` and the archived detail.
    """
    ws = tenant_a["workspace"]["id"]
    slug = tenant_a["workspace"]["slug"]
    user_a = tenant_a["user"]["id"]
    user_b = seed.user()["id"]
    seed.member(ws, user_b, role=20)
    seed.project_member(project_a, ws, user_b, role=20)
    est = estimate(db, project_a, ws)
    ep = estimate_point(db, est["id"], project_a, ws, value="3")
    try:
        with db.cursor() as cur:
            cur.execute(
                "update projects set estimate_id=%s where id=%s",
                (est["id"], project_a),
            )
        mid = seed.module(ws, project_a, name="CT1424 dist")
        iid = seed.issue(ws, project_a, name="CT1424 issue")
        with db.cursor() as cur:
            cur.execute(
                "update issues set estimate_point_id=%s where id=%s",
                (ep["id"], iid),
            )
        seed.module_issue(ws, project_a, mid, iid)
        seed.issue_assignee(ws, project_a, iid, user_a)
        lab1 = seed.label(ws, project_a, name="ct1424-old")
        seed.issue_label(ws, project_a, iid, lab1)
        lab2 = seed.label(ws, project_a, name="ct1424-new")
        issue_url = (
            f"/api/workspaces/{slug}/projects/{project_a}/issues/{iid}/"
        )
        r = api.patch(
            issue_url, headers=auth_a,
            json={"assignee_ids": [user_b], "label_ids": [lab2]},
        )
        assert r.status_code == 204, r.text
        assert r.content == b""
        with db.cursor() as cur:
            cur.execute(
                "update labels set deleted_at=now() where id=%s", (lab1,)
            )
            cur.execute(
                "select display_name from users where id=%s", (user_a,)
            )
            disp_a = cur.fetchone()[0]
            cur.execute(
                "select display_name from users where id=%s", (user_b,)
            )
            disp_b = cur.fetchone()[0]

        want_assign = {
            user_a: {
                "first_name": "Contract", "last_name": "Test",
                "assignee_id": user_a, "display_name": disp_a,
                "avatar_url": "", "total_issues": 1,
                "completed_issues": 0, "pending_issues": 1,
            },
            user_b: {
                "first_name": "Contract", "last_name": "Test",
                "assignee_id": user_b, "display_name": disp_b,
                "avatar_url": "", "total_issues": 1,
                "completed_issues": 0, "pending_issues": 1,
            },
        }
        want_assign_est = {}
        for uid, row in want_assign.items():
            est_row = {
                k: row[k] for k in (
                    "first_name", "last_name", "assignee_id",
                    "display_name", "avatar_url",
                )
            }
            est_row.update(
                total_estimates=3.0, completed_estimates=None,
                pending_estimates=3.0,
            )
            want_assign_est[uid] = est_row
        want_labels = {
            lab1: {
                "label_name": "ct1424-old", "color": "#ffffff",
                "label_id": lab1, "total_issues": 1,
                "completed_issues": 0, "pending_issues": 1,
            },
            lab2: {
                "label_name": "ct1424-new", "color": "#ffffff",
                "label_id": lab2, "total_issues": 1,
                "completed_issues": 0, "pending_issues": 1,
            },
        }
        want_labels_est = {
            lid: {
                "label_name": row["label_name"], "color": row["color"],
                "label_id": lid, "total_estimates": 3.0,
                "completed_estimates": None, "pending_estimates": 3.0,
            }
            for lid, row in want_labels.items()
        }

        r = api.get(module_url(tenant_a, project_a, mid), headers=auth_a)
        assert r.status_code == 200, r.text
        body = r.json()
        got = {row["assignee_id"]: row for row in body["distribution"]["assignees"]}
        assert got == want_assign, body["distribution"]["assignees"]
        got = {row["label_id"]: row for row in body["distribution"]["labels"]}
        assert got == want_labels, body["distribution"]["labels"]
        got = {row["assignee_id"]: row
               for row in body["estimate_distribution"]["assignees"]}
        assert got == want_assign_est, body["estimate_distribution"]["assignees"]
        got = {row["label_id"]: row
               for row in body["estimate_distribution"]["labels"]}
        assert got == want_labels_est, body["estimate_distribution"]["labels"]

        r = api.patch(
            module_url(tenant_a, project_a, mid), headers=auth_a,
            json={"status": "completed"},
        )
        assert r.status_code == 200, r.text
        r = api.post(
            f"{module_url(tenant_a, project_a, mid)}archive/", headers=auth_a,
        )
        assert r.status_code == 200, r.text
        arch_url = (
            f"/api/workspaces/{slug}/projects/{project_a}/archived-modules/{mid}/"
        )
        r = api.get(arch_url, headers=auth_a)
        assert r.status_code == 200, r.text
        arch = r.json()
        got = {row["assignee_id"]: row for row in arch["distribution"]["assignees"]}
        assert got == want_assign, arch["distribution"]["assignees"]
        got = {row["label_id"]: row for row in arch["distribution"]["labels"]}
        assert got == want_labels, arch["distribution"]["labels"]
        got = {row["assignee_id"]: row
               for row in arch["estimate_distribution"]["assignees"]}
        assert got == want_assign_est, arch["estimate_distribution"]["assignees"]
        got = {row["label_id"]: row
               for row in arch["estimate_distribution"]["labels"]}
        assert got == want_labels_est, arch["estimate_distribution"]["labels"]
    finally:
        with db.cursor() as cur:
            cur.execute(
                "update issues set estimate_point_id=null"
                " where project_id=%s",
                (project_a,),
            )
            cur.execute(
                "update projects set estimate_id=null where id=%s",
                (project_a,),
            )
            cur.execute(
                "delete from estimate_points where project_id=%s",
                (project_a,),
            )
            cur.execute(
                "delete from estimates where project_id=%s", (project_a,),
            )


def test_list_no_fields_shape(api, seed, tenant_a, project_a, auth_a):
    """Absent, empty, and comma-only `fields` all fall through to the
    annotated `.values()` row: 28 keys in SHARED_ROW_ORDER, no
    `archived_at`."""
    mid = seed.module(tenant_a["workspace"]["id"], project_a, name="Annotated")
    url = modules_url(tenant_a, project_a)
    for suffix in ("", "?fields=", "?fields=,", "?fields=,,"):
        r = api.get(url + suffix, headers=auth_a)
        assert r.status_code == 200, (suffix, r.text)
        entry = next(m for m in r.json() if m["id"] == mid)
        assert list(entry) == MODULE_VALUES_ORDER, (suffix, list(entry))
        assert "archived_at" not in entry
        assert set(entry) == MODULE_ROW_KEYS, sorted(set(entry) ^ MODULE_ROW_KEYS)


def test_retrieve_completion_chart_uses_actor_timezone(
    api, seed, db, tenant_a, project_a, auth_a,
):
    """Burndown charts bucket completions in the actor zone (PIDASHCONV-1423).

    The completion lands at 20:00 UTC two days ago, which is the next
    calendar day in Asia/Tokyo. Django's TruncDate (TimezoneMixin active
    zone) counts it on the Tokyo day, so the UTC day still shows the
    issue/points pending; a UTC truncation counts it a day early.
    """
    today = datetime.now(timezone.utc).date()
    utc_day = str(today - timedelta(days=2))
    tokyo_day = str(today - timedelta(days=1))
    completed_at = datetime.combine(
        today - timedelta(days=2), datetime.min.time()
    ).replace(tzinfo=timezone.utc) + timedelta(hours=20)
    ws = tenant_a["workspace"]["id"]
    with db.cursor() as cur:
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s",
            ("Asia/Tokyo", tenant_a["user"]["id"]),
        )
    mid = seed.module(ws, project_a, name="CT1423 burndown")
    with db.cursor() as cur:
        cur.execute(
            "UPDATE modules SET start_date = %s, target_date = %s WHERE id = %s",
            (
                (today - timedelta(days=3)).isoformat(),
                (today + timedelta(days=3)).isoformat(),
                mid,
            ),
        )
    est = estimate(db, project_a, ws)
    ep = estimate_point(db, est["id"], project_a, ws, value="5")
    try:
        issue_id = seed.issue(
            ws, project_a, name="ct1423 issue", completed_at=completed_at,
        )
        with db.cursor() as cur:
            cur.execute(
                "UPDATE projects SET estimate_id = %s WHERE id = %s",
                (est["id"], project_a),
            )
            cur.execute(
                "UPDATE issues SET estimate_point_id = %s WHERE id = %s",
                (ep["id"], issue_id),
            )
        seed.module_issue(ws, project_a, mid, issue_id)
        r = api.get(module_url(tenant_a, project_a, mid), headers=auth_a)
        assert r.status_code == 200, r.text
        body = r.json()
        issues_chart = body["distribution"]["completion_chart"]
        assert issues_chart[utc_day] == 1, issues_chart
        assert issues_chart[tokyo_day] == 0, issues_chart
        points_chart = body["estimate_distribution"]["completion_chart"]
        assert points_chart[utc_day] == 5.0, points_chart
        assert points_chart[tokyo_day] == 0.0, points_chart
    finally:
        with db.cursor() as cur:
            cur.execute(
                "UPDATE issues SET estimate_point_id = NULL WHERE project_id = %s",
                (project_a,),
            )
            cur.execute(
                "UPDATE projects SET estimate_id = NULL WHERE id = %s",
                (project_a,),
            )
            cur.execute(
                "DELETE FROM estimate_points WHERE project_id = %s", (project_a,),
            )
            cur.execute("DELETE FROM estimates WHERE project_id = %s", (project_a,))
