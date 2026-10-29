"""Non-canonical `<uuid:>` spellings on every app_cycles detail route.

Django declares every app_cycles detail path with `<uuid:...>`, whose
converter regex is lowercase-hyphenated only (Django's `UUIDConverter`,
`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}` matched
case-sensitively). Any other spelling — uppercase, simple (32 hex, no
hyphens), braced `{...}`, or `urn:uuid:...` — fails URL resolution and
answers the resolver 404 before any auth or view code runs. Rust parses
the segment with `Uuid::from_str`, which accepts all four spellings, so
without the canonical gate it serves them (real reads/writes/deletes, or
a 401/403 for anonymous callers).

PIDASHCONV-1180 mirrors the PIDASHCONV-1095 (`v1_cli_auth` runner delete)
and PIDASHCONV-1132 (`license` admin delete) fixes: on each detail route
the raw segment is checked against the converter before auth and any
non-canonical spelling proxies to Django for its own resolver 404.

`garbage` on favorite destroy is covered too: it must 404 (Django never
routes it), not answer the view's 400 to a member or the auth error to
an anonymous caller.
"""

import httpx
import pytest

from _harness import env
from .conftest import cycle_urls

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


def _detail_routes(urls, uuid_str, issue_id):
    """(name, method, url, destructive) for every app_cycles detail route
    that carries a `<uuid:>` segment."""
    return [
        ("cycles_detail_get", "GET", urls["detail"](uuid_str), False),
        ("cycles_detail_patch", "PATCH", urls["detail"](uuid_str), False),
        ("cycles_detail_delete", "DELETE", urls["detail"](uuid_str), True),
        ("cycle_issues_list", "GET", urls["cycle_issues"](uuid_str), False),
        ("cycle_issues_create", "POST", urls["cycle_issues"](uuid_str), False),
        (
            "cycle_issue_detail_delete",
            "DELETE",
            urls["cycle_issue_detail"](uuid_str, issue_id),
            True,
        ),
        (
            "cycle_issue_detail_delete_issue_seg",
            "DELETE",
            urls["cycle_issue_detail"](issue_id, uuid_str),
            True,
        ),
        ("favorite_destroy", "DELETE", urls["favorite_detail"](uuid_str), True),
        ("transfer", "POST", urls["transfer"](uuid_str), True),
        ("user_properties", "GET", urls["user_properties"](uuid_str), False),
        ("archive_post", "POST", urls["archive"](uuid_str), True),
        ("archive_delete", "DELETE", urls["archive"](uuid_str), True),
        ("archived_detail", "GET", urls["archived_detail"](uuid_str), False),
        ("progress", "GET", urls["progress"](uuid_str), False),
        ("analytics", "GET", urls["analytics"](uuid_str), False),
    ]


def _dispatch(client, method, url):
    if method == "GET":
        return client.get(url)
    if method == "DELETE":
        return client.delete(url)
    if method == "POST":
        return client.post(url, json={})
    return client.request(method, url, json={})


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_member_non_canonical_spellings_404(make_tenant, spelling):
    """A session-bearing member gets the resolver 404 for every spelling
    on every detail route (never a served response)."""
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    issue = _seed_issue(tenant)
    segment = _spellings(cid)[spelling]
    for name, method, url, _ in _detail_routes(urls, segment, issue):
        res = _dispatch(tenant["client"], method, url)
        _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_anonymous_non_canonical_spellings_404(make_tenant, spelling):
    """Resolver 404 precedes auth: anonymous callers get 404, not 401/403."""
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    issue = _seed_issue(tenant)
    segment = _spellings(cid)[spelling]
    with httpx.Client(base_url=env.base_url(), timeout=30) as anon:
        for name, method, url, _ in _detail_routes(urls, segment, issue):
            res = _dispatch(anon, method, url)
            _assert_resolver_404(res)


def test_non_canonical_delete_leaves_cycle_live(make_tenant):
    """A non-canonical DELETE of a live cycle is never routed: no write."""
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    res = tenant["client"].delete(urls["detail"](cid.upper()))
    _assert_resolver_404(res)
    assert _cycle_live(tenant["conn"], cid), "cycle must not be soft-deleted"


def test_non_canonical_favorite_destroy_leaves_row(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    favorite_id = _seed_favorite(tenant, cid)
    res = tenant["client"].delete(urls["favorite_detail"](cid.upper()))
    _assert_resolver_404(res)
    assert _favorite_live(tenant["conn"], favorite_id), "favorite must remain"


def test_non_canonical_archive_leaves_cycle_unarchived(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    res = tenant["client"].post(urls["archive"](cid.upper()))
    _assert_resolver_404(res)
    assert _archived_at(tenant["conn"], cid) is None


def test_garbage_favorite_destroy_404s_member(make_tenant):
    """Garbage on favorite destroy is a resolver 404, not the view's 400."""
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    res = tenant["client"].delete(urls["favorite_detail"]("not-a-uuid"))
    _assert_resolver_404(res)


def test_garbage_favorite_destroy_404s_anonymous(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    with httpx.Client(base_url=env.base_url(), timeout=30) as anon:
        res = anon.delete(urls["favorite_detail"]("not-a-uuid"))
    _assert_resolver_404(res)


# -- DB helpers (straight into Postgres, the suite's convention) ------------


def _cycle_live(conn, cid):
    with conn.cursor() as cur:
        cur.execute(
            "select count(*) from cycles where id = %s and deleted_at is null",
            (cid,),
        )
        return cur.fetchone()[0] == 1


def _seed_favorite(tenant, cid):
    import uuid

    fid = str(uuid.uuid4())
    with tenant["conn"].cursor() as cur:
        cur.execute(
            """insert into user_favorites (id, workspace_id, project_id, user_id,
                   entity_type, entity_identifier, is_folder, sequence,
                   created_at, updated_at)
               values (%s,%s,%s,%s,'cycle',%s,false,65535,now(),now())""",
            (
                fid,
                tenant["workspace"]["id"],
                tenant["project"]["id"],
                tenant["user"]["id"],
                cid,
            ),
        )
    tenant["seed"].track("user_favorites", "id", fid)
    return fid


def _favorite_live(conn, fid):
    with conn.cursor() as cur:
        cur.execute(
            "select count(*) from user_favorites where id = %s and deleted_at is null",
            (fid,),
        )
        return cur.fetchone()[0] == 1


def _archived_at(conn, cid):
    with conn.cursor() as cur:
        cur.execute("select archived_at from cycles where id = %s", (cid,))
        return cur.fetchone()[0]


def _seed_issue(tenant):
    return tenant["seed"].issue(
        tenant["workspace"]["id"],
        tenant["project"]["id"],
        name="uuid-paths",
        state_id=tenant["state"]["id"],
    )
