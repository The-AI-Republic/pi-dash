"""Favorites + user properties (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/user-favorite-modules/
  DELETE .../user-favorite-modules/<module_id>/
  GET/PATCH .../modules/<module_id>/user-properties/

Port-existing bug (pinned, see PR): GET on the favorites list 500s —
ModuleFavoriteViewSet declares no serializer_class, so DRF's default
list action raises into the generic 500 handler. Create/destroy are
custom actions and work.
"""

from .conftest import modules_url

SERVER_ERROR = {"error": "Something went wrong please try again later"}

PROPS_KEYS = {
    "id", "created_at", "updated_at", "deleted_at", "filters",
    "display_filters", "display_properties", "rich_filters",
    "created_by", "updated_by", "project", "workspace", "module", "user",
}

DEFAULT_FILTERS = {
    "priority": None, "state": None, "state_group": None, "assignees": None,
    "created_by": None, "labels": None, "start_date": None,
    "target_date": None, "subscriber": None,
}

DEFAULT_DISPLAY_FILTERS = {
    "group_by": None, "order_by": "-created_at", "type": None,
    "sub_issue": True, "show_empty_groups": True, "layout": "list",
    "calendar_date_range": "",
}


def _favorites_url(tenant, project_id):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{project_id}/user-favorite-modules/"
    )


def _favorite_url(tenant, project_id, module_id):
    return f"{_favorites_url(tenant, project_id)}{module_id}/"


def _props_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/user-properties/"


def test_favorite_create(api, seed, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _favorites_url(tenant_a, project_a), headers=auth_a,
        json={"module": module_a},
    )
    assert r.status_code == 204, r.text
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM user_favorites WHERE entity_type = 'module'"
            " AND entity_identifier = %s AND user_id = %s AND deleted_at IS NULL",
            (module_a, tenant_a["user"]["id"]),
        )
        assert cur.fetchone()[0] == 1


def test_favorite_duplicate_rejected(api, seed, tenant_a, project_a, auth_a, module_a):
    seed.favorite_module(
        tenant_a["workspace"]["id"], project_a, tenant_a["user"]["id"], module_a
    )
    r = api.post(
        _favorites_url(tenant_a, project_a), headers=auth_a,
        json={"module": module_a},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "The payload is not valid"}


def test_favorite_list_is_server_error(api, tenant_a, project_a, auth_a, module_a):
    """Port-existing bug: no serializer_class on the viewset, so the
    default list action raises AssertionError into the 500 handler."""
    r = api.get(_favorites_url(tenant_a, project_a), headers=auth_a)
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_favorite_destroy(api, seed, tenant_a, project_a, auth_a, module_a):
    seed.favorite_module(
        tenant_a["workspace"]["id"], project_a, tenant_a["user"]["id"], module_a
    )
    r = api.delete(_favorite_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 204, r.text


def test_favorite_destroy_missing(api, tenant_a, project_a, auth_a, module_a):
    r = api.delete(_favorite_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "The required object does not exist."}


def test_user_properties_get_creates_with_defaults(api, tenant_a, project_a, auth_a, module_a):
    r = api.get(_props_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == PROPS_KEYS, sorted(set(body) ^ PROPS_KEYS)
    assert body["filters"] == DEFAULT_FILTERS
    assert body["display_filters"] == DEFAULT_DISPLAY_FILTERS
    assert body["rich_filters"] == {}
    assert body["module"] == module_a
    assert body["user"] == tenant_a["user"]["id"]


def test_user_properties_patch(api, tenant_a, project_a, auth_a, module_a):
    api.get(_props_url(tenant_a, project_a, module_a), headers=auth_a)
    r = api.patch(
        _props_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"filters": {"priority": "high"}},
    )
    # Port-existing quirk: PATCH answers 201, not 200.
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body) == PROPS_KEYS, sorted(set(body) ^ PROPS_KEYS)
    assert body["filters"] == {"priority": "high"}


def test_user_properties_patch_without_row(api, seed, tenant_a, project_a, auth_a, module_a):
    """PATCH uses .get(), so without a prior GET (which get_or_creates)
    it 404s through the DoesNotExist handler."""
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"], role=20)
    seed.project_member(project_a, tenant_a["workspace"]["id"], other["id"], role=20)
    import os
    from conftest import session_headers
    other_headers = session_headers(
        seed, other, other["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.patch(
        _props_url(tenant_a, project_a, module_a), headers=other_headers,
        json={"filters": {"priority": "high"}},
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "The required object does not exist."}
