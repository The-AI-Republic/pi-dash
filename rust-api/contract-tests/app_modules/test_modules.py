"""Modules CRUD (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/modules/
  GET/PUT/PATCH/DELETE /api/workspaces/<slug>/projects/<project_id>/modules/<pk>/

PUT has no custom action: it falls back to DRF's default update over
ModuleWriteSerializer, so its key set differs from the annotated
create/PATCH shapes. Pinned here on purpose.
"""

from conftest import (
    MODULE_DETAIL_KEYS as DETAIL_KEYS,
    MODULE_ROW_KEYS,
    module_url,
    modules_url,
    session_headers,
)

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
