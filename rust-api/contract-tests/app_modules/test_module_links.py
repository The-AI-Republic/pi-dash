"""Module links (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  GET/POST /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/
  GET/PUT/PATCH/DELETE .../module-links/<pk>/

Quirks pinned: bare domains gain an http:// prefix before validation;
PATCH without a url is rejected with {"error": "Invalid URL format."}
(the update path validates None); duplicate urls are rejected per
module.
"""

from .conftest import modules_url

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


def test_create_sets_location_to_link_url(api, tenant_a, project_a, auth_a, module_a):
    # DRF quirk (PIDASHCONV-873): ModuleLinkViewSet has no custom create,
    # so CreateModelMixin answers 201 with Location: <data['url']> — the
    # created link's target URL, not a resource URL.
    r = api.post(
        _links_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"title": "Specs", "url": "https://example.com/specs"},
    )
    assert r.status_code == 201, r.text
    assert r.headers["location"] == "https://example.com/specs"
    assert r.headers["location"] == r.json()["url"]


def test_create_non_latin1_url_location_is_mime_encoded(api, tenant_a, project_a, auth_a, module_a):
    # Django ResponseHeaders MIME-encodes (RFC 2047) header values past
    # latin-1 (PIDASHCONV-880). Both encodings are pinned: the shorter URL
    # picks base64, the longer one ties SHORTEST and picks Q. The output is
    # pure ASCII, so the string comparison is byte-exact on both backends.
    cases = [
        (
            "https://example.com/\U0001F600",
            "=?utf-8?b?aHR0cHM6Ly9leGFtcGxlLmNvbS/wn5iA?=",
        ),
        (
            "https://example.com/django9/\U0001F600",
            "=?utf-8?q?https=3A//example=2Ecom/django9/=F0=9F=98=80?=",
        ),
    ]
    for url, encoded in cases:
        r = api.post(
            _links_url(tenant_a, project_a, module_a), headers=auth_a,
            json={"title": "Emoji", "url": url},
        )
        assert r.status_code == 201, r.text
        assert r.json()["url"] == url
        assert r.headers["location"] == encoded, url


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


def test_create_url_with_at_in_query_or_fragment_accepted(api, tenant_a, project_a, auth_a, module_a):
    # PIDASHCONV-1425: an `@` past the netloc is not userinfo — urlsplit
    # ends the authority at the first `/`, `?` or `#`, and Django's
    # URLValidator userinfo group backtracks to the no-userinfo reading.
    cases = [
        "https://example.com?email=user@gmail.com&ref=x",
        "https://example.com#user@gmail.com",
        "http://example.com:8080?x=a@b",
    ]
    for url in cases:
        r = api.post(
            _links_url(tenant_a, project_a, module_a), headers=auth_a,
            json={"title": "Tracked", "url": url},
        )
        assert r.status_code == 201, (url, r.text)
        assert r.json()["url"] == url, url


def test_create_nfkc_lookalike_netloc_rejected(api, tenant_a, project_a, auth_a, module_a):
    # PIDASHCONV-1425: urlsplit raises ValueError when NFKC maps a netloc
    # char onto a delimiter (fullwidth solidus U+FF0F, fullwidth
    # commercial at U+FF20), so Django rejects these.
    cases = [
        "https://example\uff0fevil.com",
        "https://a.com\uff20b.com",
    ]
    for url in cases:
        r = api.post(
            _links_url(tenant_a, project_a, module_a), headers=auth_a,
            json={"title": "Lookalike", "url": url},
        )
        assert r.status_code == 400, (url, r.text)
        assert r.json() == {"url": ["Enter a valid URL."]}, url
