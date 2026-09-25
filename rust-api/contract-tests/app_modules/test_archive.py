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

from .conftest import module_url, modules_url

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
    from conftest import MODULE_DETAIL_KEYS as DETAIL_KEYS
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
