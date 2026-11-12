"""Non-canonical `<uuid:>` spellings on app_modules routes.

Django declares every app_modules detail path with `<uuid:...>`, whose
converter regex is lowercase-hyphenated only (Django's `UUIDConverter`,
`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}` matched
case-sensitively). Any other spelling — uppercase, simple (32 hex, no
hyphens), braced `{...}`, or `urn:uuid:...` — fails URL resolution and
answers the resolver 404 before any auth or view code runs. Rust parsed
the segment with `Uuid::from_str` (via `parse_uuid_or_invalid`), which
accepts all four spellings, so without the canonical gate it served them
(real reads/writes/deletes, or a 401/403 for anonymous callers).

PIDASHCONV-1712 mirrors the PIDASHCONV-1180 (`app_cycles`, fixed with
`is_canonical_uuid` plus proxy) precedent: on each app_modules detail
route the raw segment is checked with `crate::uuid_path::classify_uuid_segment`
before auth and any parseable non-canonical spelling proxies to Django
for its own resolver 404. Garbage keeps the local 400
`{"error": "Please provide valid detail"}` (deliberate local choice).
"""

import httpx
import pytest

from _harness import env
from .conftest import module_url, modules_url

pytestmark = pytest.mark.contract


def _spellings(uuid_str):
    """Every spelling `uuid.UUID()` (and Rust's `Uuid::parse_str`) accepts
    but Django's `<uuid:>` converter does not route."""
    return {
        "uppercase": uuid_str.upper(),
        "simple": uuid_str.replace("-", ""),
        "braced": "{" + uuid_str + "}",
        "urn": "urn:uuid:" + uuid_str,
    }


def _assert_resolver_404(res):
    # Resolving the URL precedes auth and the view entirely; the live
    # Django runs with DEBUG on (pi_dash.settings.test), so the answer is
    # the technical 404 page: status 404 + text/html. Never a JSON body,
    # never 200/201/204, never 400/401/403.
    assert res.status_code == 404, res.text[:400]
    assert res.headers["content-type"].startswith("text/html"), res.headers.get(
        "content-type"
    )


def _module_issues_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/issues/"


def _module_issue_url(tenant, project_id, module_id, issue_id):
    return f"{_module_issues_url(tenant, project_id, module_id)}{issue_id}/"


def _detail_routes(tenant, project_id, uuid_str, issue_id):
    """(name, method, url) for every app_modules detail route that carries a
    `<uuid:>` segment (required by the issue's Done criteria plus the
    full slice for completeness)."""
    return [
        ("module_retrieve", "GET", module_url(tenant, project_id, uuid_str)),
        ("module_patch", "PATCH", module_url(tenant, project_id, uuid_str)),
        ("module_delete", "DELETE", module_url(tenant, project_id, uuid_str)),
        (
            "module_issues_create",
            "POST",
            _module_issues_url(tenant, project_id, uuid_str),
        ),
        (
            "module_issue_delete",
            "DELETE",
            _module_issue_url(tenant, project_id, uuid_str, issue_id),
        ),
        (
            "module_issue_delete_issue_seg",
            "DELETE",
            _module_issue_url(tenant, project_id, issue_id, uuid_str),
        ),
    ]


def _dispatch(client, method, url, json=None):
    if method == "GET":
        return client.get(url)
    if method == "DELETE":
        return client.delete(url)
    if method == "POST":
        return client.post(url, json=json or {})
    if method == "PUT":
        return client.put(url, json=json or {})
    return client.request(method, url, json=json or {})


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_member_non_canonical_spellings_404(api, tenant_a, project_a, auth_a, module_a, issue_a, spelling):
    """A session-bearing project admin gets the resolver 404 for every spelling
    on every required detail route (never a served response)."""
    segment = _spellings(module_a)[spelling]
    routes = _detail_routes(tenant_a, project_a, segment, issue_a)
    for name, method, url in routes:
        if method == "POST":
            # module-issues create needs a valid issues body
            res = api.post(url, headers=auth_a, json={"issues": [issue_a]})
        elif method == "PATCH":
            res = api.patch(url, headers=auth_a, json={"name": "x"})
        else:
            res = api.request(method, url, headers=auth_a)
        _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_member_uppercase_simple_required_routes_404(api, tenant_a, project_a, auth_a, module_a, issue_a, spelling):
    """The issue's Done criteria: uppercase and simple-form module ids to
    module retrieve/PATCH/DELETE and to module-issues POST/DELETE."""
    segment = _spellings(module_a)[spelling]
    cases = [
        ("GET", module_url(tenant_a, project_a, segment), None),
        ("PATCH", module_url(tenant_a, project_a, segment), {"name": "x"}),
        ("DELETE", module_url(tenant_a, project_a, segment), None),
        ("POST", _module_issues_url(tenant_a, project_a, segment), {"issues": [issue_a]}),
        ("DELETE", _module_issue_url(tenant_a, project_a, segment, issue_a), None),
    ]
    for method, url, payload in cases:
        if payload is None and method == "GET":
            res = api.get(url, headers=auth_a)
        elif payload is None and method == "DELETE":
            res = api.delete(url, headers=auth_a)
        elif method == "PATCH":
            res = api.patch(url, headers=auth_a, json=payload)
        else:
            res = api.post(url, headers=auth_a, json=payload)
        _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_anonymous_non_canonical_spellings_404(api, tenant_a, project_a, module_a, issue_a, spelling):
    """Resolver 404 precedes auth: anonymous callers get 404, not 401/403."""
    segment = _spellings(module_a)[spelling]
    routes = _detail_routes(tenant_a, project_a, segment, issue_a)
    with httpx.Client(base_url=env.base_url(), timeout=30) as anon:
        for name, method, url in routes:
            if method == "POST":
                res = anon.post(url, json={"issues": [issue_a]})
            elif method == "PATCH":
                res = anon.patch(url, json={"name": "x"})
            else:
                res = anon.request(method, url)
            _assert_resolver_404(res)


def test_non_canonical_delete_leaves_module_live(api, seed, tenant_a, project_a, auth_a, module_a):
    """A non-canonical DELETE of a live module is never routed: no write."""
    segment = _spellings(module_a)["uppercase"]
    res = api.delete(module_url(tenant_a, project_a, segment), headers=auth_a)
    _assert_resolver_404(res)
    assert _module_live(seed.conn, module_a), "module must not be soft-deleted"


def test_non_canonical_patch_leaves_name(api, seed, tenant_a, project_a, auth_a, module_a):
    """A non-canonical PATCH never writes."""
    segment = _spellings(module_a)["simple"]
    before = _module_name(seed.conn, module_a)
    res = api.patch(
        module_url(tenant_a, project_a, segment),
        headers=auth_a,
        json={"name": "should-not-write"},
    )
    _assert_resolver_404(res)
    assert _module_name(seed.conn, module_a) == before


def test_non_canonical_create_module_issues_creates_no_bridge(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    """POST with a non-canonical module id creates no bridge rows."""
    segment = _spellings(module_a)["uppercase"]
    # ensure clean slate
    with seed.conn.cursor() as cur:
        cur.execute(
            "DELETE FROM module_issues WHERE module_id = %s AND issue_id = %s",
            (module_a, issue_a),
        )
    res = api.post(
        _module_issues_url(tenant_a, project_a, segment),
        headers=auth_a,
        json={"issues": [issue_a]},
    )
    _assert_resolver_404(res)
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 0, "no bridge row must be created for non-canonical id"


def test_non_canonical_module_issue_delete_leaves_bridge(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    """DELETE with a non-canonical module id leaves the bridge live."""
    # create a live bridge first via canonical id
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a),
        headers=auth_a,
        json={"issues": [issue_a]},
    )
    assert r.status_code == 201, r.text
    segment = _spellings(module_a)["simple"]
    res = api.delete(
        _module_issue_url(tenant_a, project_a, segment, issue_a),
        headers=auth_a,
    )
    _assert_resolver_404(res)
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 1, "bridge must remain live after non-canonical DELETE"


# NOTE: Garbage (not parseable as UUID) is a deliberate Tier-3 divergence:
# Django resolver-404s before auth (404 text/html) while Rust answers the
# view's 400 `{"error":"Please provide valid detail"}` after auth
# (`parse_uuid_or_invalid`). The issue documents this as out-of-scope,
# so it is not asserted here — the required Done criteria only cover
# parseable non-canonical spellings.


# -- DB helpers (straight into Postgres, the suite's convention) ------------


def _module_live(conn, mid):
    with conn.cursor() as cur:
        cur.execute(
            "select count(*) from modules where id = %s and deleted_at is null",
            (mid,),
        )
        return cur.fetchone()[0] == 1


def _module_name(conn, mid):
    with conn.cursor() as cur:
        cur.execute("select name from modules where id = %s", (mid,))
        return cur.fetchone()[0]
