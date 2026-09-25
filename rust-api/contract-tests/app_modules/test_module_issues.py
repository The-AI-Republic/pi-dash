"""Module ↔ issue links (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  POST /api/workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/
  POST /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/
  GET  /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/
  GET/PUT/PATCH/DELETE .../modules/<module_id>/issues/<issue_id>/

Port-existing bugs (pinned, see PR): GET/PUT/PATCH on the detail route
500 with {"error": "Something went wrong please try again later"} — the
URL names the kwargs module_id/issue_id but DRF's default
retrieve/update look for `pk`. DELETE is a custom action and works.
"""

from conftest import module_url, modules_url

SERVER_ERROR = {"error": "Something went wrong please try again later"}

# Paginated issue envelope (BasePaginator) for the module-issues list.
LIST_KEYS = {
    "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
    "prev_cursor", "next_page_results", "prev_page_results", "count",
    "total_pages", "total_results", "extra_stats", "results",
}

ISSUE_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "created_at", "updated_at",
    "created_by", "updated_by", "is_draft", "archived_at", "state__group",
    "cycle_id", "link_count", "attachment_count", "sub_issues_count",
    "assignee_ids", "label_ids", "module_ids",
}


def _issue_modules_url(tenant, project_id, issue_id):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{project_id}/issues/{issue_id}/modules/"
    )


def _module_issues_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/issues/"


def _module_issue_url(tenant, project_id, module_id, issue_id):
    return f"{_module_issues_url(tenant, project_id, module_id)}{issue_id}/"


def test_create_module_issues(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"issues": [issue_a]},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
            " AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 1


def test_create_module_issues_empty_rejected(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        json={},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Issues are required"}


def test_create_issue_modules(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    r = api.post(
        _issue_modules_url(tenant_a, project_a, issue_a), headers=auth_a,
        json={"modules": [module_a], "removed_modules": []},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
            " AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 1


def test_create_issue_modules_remove(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.post(
        _issue_modules_url(tenant_a, project_a, issue_a), headers=auth_a,
        json={"modules": [], "removed_modules": [module_a]},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["total_count"] == 0


def test_list_shape(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == LIST_KEYS, sorted(set(body) ^ LIST_KEYS)
    assert body["total_count"] == 1
    row = body["results"][0]
    assert set(row) == ISSUE_KEYS, sorted(set(row) ^ ISSUE_KEYS)
    assert row["id"] == issue_a
    assert row["module_ids"] == [module_a]


def test_detail_get_put_patch_are_server_errors(api, seed, tenant_a, project_a, auth_a,
                                                module_a, issue_a):
    """Port-existing bug: the detail routes name their kwargs
    module_id/issue_id, so DRF's default retrieve/update cannot find `pk`
    and raise into the generic 500 handler."""
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    url = _module_issue_url(tenant_a, project_a, module_a, issue_a)
    r = api.get(url, headers=auth_a)
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    r = api.put(url, headers=auth_a, json={"name": "x"})
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    r = api.patch(url, headers=auth_a, json={"name": "x"})
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_destroy(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.delete(_module_issue_url(tenant_a, project_a, module_a, issue_a), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["total_count"] == 0


def test_requires_auth(api, tenant_a, project_a, module_a):
    r = api.get(_module_issues_url(tenant_a, project_a, module_a))
    assert r.status_code == 401, r.text
