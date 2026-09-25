"""Module links (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/
  GET/PUT/PATCH/DELETE .../module-links/<pk>/

Quirks pinned: bare domains gain an http:// prefix before validation;
PATCH without a url is rejected with {"error": "Invalid URL format."}
(the update path validates None); duplicate urls are rejected per
module.
"""

from conftest import modules_url

LINK_KEYS = {
    "id", "created_at", "updated_at", "deleted_at", "title", "url",
    "metadata", "created_by", "updated_by", "project", "workspace", "module",
}


def _links_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/module-links/"


def _link_url(tenant, project_id, module_id, pk):
    return f"{_links_url(tenant, project_id, module_id)}{pk}/"


def test_create_shape(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _links_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"title": "Specs", "url": "https://example.com/specs"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body) == LINK_KEYS, sorted(set(body) ^ LINK_KEYS)
    assert body["title"] == "Specs"
    assert body["url"] == "https://example.com/specs"
    assert body["module"] == module_a
    assert body["metadata"] == {}


def test_create_bare_domain_gets_http_prefix(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _links_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"title": "Docs", "url": "example.com/docs"},
    )
    assert r.status_code == 201, r.text
    assert r.json()["url"] == "http://example.com/docs"


def test_create_invalid_url_rejected(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _links_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"title": "Bad", "url": "not a url at all !!"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"url": ["Enter a valid URL."]}


def test_create_duplicate_url_rejected(api, seed, tenant_a, project_a, auth_a, module_a):
    seed.module_link(
        tenant_a["workspace"]["id"], project_a, module_a,
        url="http://example.com/dup",
    )
    r = api.post(
        _links_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"title": "Dup", "url": "http://example.com/dup"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "URL already exists."}


def test_list_shape(api, seed, tenant_a, project_a, auth_a, module_a):
    lid = seed.module_link(tenant_a["workspace"]["id"], project_a, module_a)
    r = api.get(_links_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list)
    entry = next(link for link in body if link["id"] == lid)
    assert set(entry) == LINK_KEYS, sorted(set(entry) ^ LINK_KEYS)


def test_retrieve_shape(api, seed, tenant_a, project_a, auth_a, module_a):
    lid = seed.module_link(
        tenant_a["workspace"]["id"], project_a, module_a, title="Detail",
    )
    r = api.get(_link_url(tenant_a, project_a, module_a, lid), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == LINK_KEYS, sorted(set(body) ^ LINK_KEYS)
    assert body["id"] == lid
    assert body["title"] == "Detail"


def test_patch_without_url_rejected(api, seed, tenant_a, project_a, auth_a, module_a):
    """Port-existing quirk: the update path unconditionally validates the
    url field, so omitting it fails with Invalid URL format."""
    lid = seed.module_link(tenant_a["workspace"]["id"], project_a, module_a)
    r = api.patch(
        _link_url(tenant_a, project_a, module_a, lid), headers=auth_a,
        json={"title": "Renamed"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Invalid URL format."}


def test_put_shape(api, seed, tenant_a, project_a, auth_a, module_a):
    lid = seed.module_link(tenant_a["workspace"]["id"], project_a, module_a)
    r = api.put(
        _link_url(tenant_a, project_a, module_a, lid), headers=auth_a,
        json={"title": "Replaced", "url": "http://example.com/replaced"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == LINK_KEYS, sorted(set(body) ^ LINK_KEYS)
    assert body["title"] == "Replaced"
    assert body["url"] == "http://example.com/replaced"


def test_destroy(api, seed, tenant_a, project_a, auth_a, module_a):
    lid = seed.module_link(tenant_a["workspace"]["id"], project_a, module_a)
    r = api.delete(_link_url(tenant_a, project_a, module_a, lid), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(_links_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert all(link["id"] != lid for link in r.json())


def test_requires_auth(api, tenant_a, project_a, module_a):
    r = api.get(_links_url(tenant_a, project_a, module_a))
    assert r.status_code == 401, r.text
