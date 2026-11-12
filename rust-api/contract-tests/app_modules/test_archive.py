"""Archive / unarchive + archived list (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  POST/DELETE /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/
  GET /api/workspaces/<slug>/projects/<project_id>/archived-modules/
  GET /api/workspaces/<slug>/projects/<project_id>/archived-modules/<pk>/

Port-existing bug (pinned, see PR): GET on the /archive/ path 500s —
ModuleArchiveUnarchiveEndpoint.get() takes no module_id kwarg, so
Django raises TypeError into the generic 500 handler. Archived modules
vanish from the main list/retrieve and appear under archived-modules.
"""

from datetime import datetime, timedelta, timezone

from .conftest import module_url, modules_url

from _harness.seed import estimate, estimate_point

SERVER_ERROR = {"error": "Something went wrong please try again later"}

# Archived list rows render a narrower .values() dict than the main
# list: no logo_props, no estimate-point annotations, plus archived_at.
ARCHIVED_ROW_KEYS = {
    "id", "workspace_id", "project_id",
    "name", "description", "description_text", "description_html",
    "start_date", "target_date", "status", "lead_id", "view_props",
    "sort_order", "external_source", "external_id",
    "created_at", "updated_at", "archived_at",
    "is_favorite", "completed_issues", "cancelled_issues", "started_issues",
    "unstarted_issues", "backlog_issues", "total_issues", "member_ids",
}


def _archive_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/archive/"


def _archived_url(tenant, project_id):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{project_id}/archived-modules/"
    )


def test_archive_planned_rejected(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(_archive_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Only completed or cancelled modules can be archived"}


def test_archive_completed(api, tenant_a, project_a, auth_a, module_a):
    api.patch(
        module_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"status": "completed"},
    )
    r = api.post(_archive_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert "archived_at" in r.json()


def test_archive_get_on_module_path_is_server_error(api, tenant_a, project_a, auth_a, module_a):
    """Port-existing bug: get() accepts (slug, project_id, pk=None) but
    the /archive/ route passes module_id, raising TypeError."""
    r = api.get(_archive_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_archived_hidden_from_main_endpoints(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Gone",
        status="completed",
    )
    api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    r = api.get(module_url(tenant_a, project_a, mid), headers=auth_a)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "Module not found"}
    r = api.get(modules_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert all(m["id"] != mid for m in r.json())


def test_archived_list_shape(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Archived one",
        status="cancelled",
    )
    api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    r = api.get(_archived_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    entry = next(m for m in body if m["id"] == mid)
    assert set(entry) == ARCHIVED_ROW_KEYS, sorted(set(entry) ^ ARCHIVED_ROW_KEYS)
    assert entry["archived_at"] is not None


def test_archived_detail_shape(api, seed, tenant_a, project_a, auth_a):
    from app_modules.conftest import MODULE_DETAIL_KEYS as DETAIL_KEYS
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Archived detail",
        status="completed",
    )
    api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    r = api.get(f"{_archived_url(tenant_a, project_a)}{mid}/", headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == DETAIL_KEYS, sorted(set(body) ^ DETAIL_KEYS)
    assert body["id"] == mid
    assert body["archived_at"] is not None


def test_patch_archived_rejected(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Frozen",
        status="completed",
    )
    api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    r = api.patch(
        module_url(tenant_a, project_a, mid), headers=auth_a,
        json={"name": "Nope"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Archived module cannot be updated"}


def test_unarchive(api, seed, tenant_a, project_a, auth_a):
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Back",
        status="cancelled",
    )
    api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    r = api.delete(_archive_url(tenant_a, project_a, mid), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(module_url(tenant_a, project_a, mid), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["archived_at"] is None


def test_archived_detail_points_estimate_labels(api, seed, db, tenant_a,
                                                project_a, auth_a):
    """Points estimates keep archived detail at 200 (PIDASHCONV-1420).

    The project carries a points estimate, so the detail renders the
    estimate_distribution block (archive.py:321-430). The Rust label query
    aliased the issues table while filtering on its bare name, 500ing every
    archived detail on such projects.
    """
    ws = tenant_a["workspace"]["id"]
    est = estimate(db, project_a, ws)
    ep = estimate_point(db, est["id"], project_a, ws, value="5")
    try:
        with db.cursor() as cur:
            cur.execute(
                "update projects set estimate_id=%s where id=%s",
                (est["id"], project_a),
            )
        label_id = seed.label(ws, project_a, name="ct1420-label")
        issue_id = seed.issue(ws, project_a, name="ct1420 issue")
        with db.cursor() as cur:
            cur.execute(
                "update issues set estimate_point_id=%s where id=%s",
                (ep["id"], issue_id),
            )
        mid = seed.module(
            ws, project_a, name="CT1420 archived", status="completed",
        )
        seed.module_issue(ws, project_a, mid, issue_id)
        seed.issue_label(ws, project_a, issue_id, label_id)
        r = api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
        assert r.status_code == 200, r.text
        r = api.get(
            f"{_archived_url(tenant_a, project_a)}{mid}/", headers=auth_a,
        )
        assert r.status_code == 200, r.text
        labels = r.json()["estimate_distribution"]["labels"]
        assert labels == [{
            "label_name": "ct1420-label",
            "color": "#ffffff",
            "label_id": label_id,
            "total_estimates": 5.0,
            "completed_estimates": None,
            "pending_estimates": 5.0,
        }], labels
    finally:
        _drop_estimates(db, project_a)


def _drop_estimates(db, project_id):
    # estimates/estimate_points are untracked by Seed.cleanup, and both
    # projects.estimate_id and issues.estimate_point_id pin them: clear
    # the references first. Project-scoped so partial seeds still clean.
    with db.cursor() as cur:
        cur.execute(
            "update issues set estimate_point_id=null"
            " where project_id=%s",
            (project_id,),
        )
        cur.execute(
            "update projects set estimate_id=null where id=%s",
            (project_id,),
        )
        cur.execute(
            "delete from estimate_points where project_id=%s",
            (project_id,),
        )
        cur.execute(
            "delete from estimates where project_id=%s", (project_id,),
        )


def test_archived_detail_completion_chart_uses_actor_timezone(
    api, seed, db, tenant_a, project_a, auth_a,
):
    """Archived burndown charts bucket completions in the actor zone too.

    Same Tokyo seed as the live retrieve test (PIDASHCONV-1423): the
    completion lands at 20:00 UTC two days ago, the next calendar day in
    Asia/Tokyo, so the UTC day still shows it pending.
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
    mid = seed.module(ws, project_a, name="CT1423 archived", status="completed")
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
        r = api.post(_archive_url(tenant_a, project_a, mid), headers=auth_a)
        assert r.status_code == 200, r.text
        r = api.get(
            f"{_archived_url(tenant_a, project_a)}{mid}/", headers=auth_a,
        )
        assert r.status_code == 200, r.text
        body = r.json()
        issues_chart = body["distribution"]["completion_chart"]
        assert issues_chart[utc_day] == 1, issues_chart
        assert issues_chart[tokyo_day] == 0, issues_chart
        points_chart = body["estimate_distribution"]["completion_chart"]
        assert points_chart[utc_day] == 5.0, points_chart
        assert points_chart[tokyo_day] == 0.0, points_chart
    finally:
        _drop_estimates(db, project_a)
