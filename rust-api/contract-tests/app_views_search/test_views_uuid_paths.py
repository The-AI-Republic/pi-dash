"""Non-canonical `<uuid:>` spellings on the view + favorite detail routes.

Django declares the view detail paths with `<uuid:pk>` and the favorite
destroy with `<uuid:view_id>` (`apps/api/pi_dash/app/urls/views.py`), whose
converter regex is lowercase-hyphenated only (Django's `UUIDConverter`,
matched case-sensitively). Any other spelling — uppercase, simple (32 hex,
no hyphens) — fails URL resolution and answers the resolver 404 before any
auth or view code runs. Rust gated those paths with `Uuid::parse_str`,
which accepts both spellings, so without the canonical gate it serves them
(real reads, PATCH/PUT writes, DELETE soft-deletes, favorite hard-deletes).

PIDASHCONV-1448 mirrors the PIDASHCONV-1180 (`app_cycles`) fix: each detail
handler classifies the raw segment with `uuid_path::classify_uuid_segment`
before auth and proxies any non-canonical spelling to Django for its own
resolver 404.
"""

import httpx
import pytest

from _harness import env

pytestmark = pytest.mark.contract


def _spellings(uuid_str):
    """Spellings `uuid.UUID()` (and Rust's `Uuid::parse_str`) accepts but
    Django's `<uuid:>` converter does not route."""
    return {
        "uppercase": uuid_str.upper(),
        "simple": uuid_str.replace("-", ""),
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


def _ws_url(slug, segment):
    return f"/api/workspaces/{slug}/views/{segment}/"


def _pr_url(slug, project_id, segment):
    return f"/api/workspaces/{slug}/projects/{project_id}/views/{segment}/"


def _fav_url(slug, project_id, segment):
    return (
        f"/api/workspaces/{slug}/projects/{project_id}/"
        f"user-favorite-views/{segment}/"
    )


def _dispatch(client, method, url, headers):
    if method == "GET":
        return client.get(url, headers=headers)
    if method == "DELETE":
        return client.delete(url, headers=headers)
    return client.request(method, url, headers=headers, json={"name": "NC-PROBE", "filters": {}})


def _detail_routes(slug, project_id, segment):
    """(name, method, url) for every detail route carrying the `<uuid:>`
    view segment."""
    return [
        ("ws_get", "GET", _ws_url(slug, segment)),
        ("ws_patch", "PATCH", _ws_url(slug, segment)),
        ("ws_put", "PUT", _ws_url(slug, segment)),
        ("ws_delete", "DELETE", _ws_url(slug, segment)),
        ("pr_get", "GET", _pr_url(slug, project_id, segment)),
        ("pr_patch", "PATCH", _pr_url(slug, project_id, segment)),
        ("pr_put", "PUT", _pr_url(slug, project_id, segment)),
        ("pr_delete", "DELETE", _pr_url(slug, project_id, segment)),
        ("fav_delete", "DELETE", _fav_url(slug, project_id, segment)),
    ]


def _view_row(db, view_id):
    with db.cursor() as cur:
        cur.execute(
            "SELECT name, deleted_at IS NOT NULL FROM issue_views WHERE id = %s",
            (view_id,),
        )
        row = cur.fetchone()
        return (row[0], bool(row[1])) if row else None


def _favorite_live(db, favorite_id):
    with db.cursor() as cur:
        cur.execute(
            "SELECT deleted_at IS NULL FROM user_favorites WHERE id = %s",
            (favorite_id,),
        )
        row = cur.fetchone()
        return bool(row[0]) if row else False


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_member_non_canonical_spellings_404(
    api, seed, tenant_a, auth_a, project_a, spelling
):
    """A session-bearing member gets the resolver 404 for both spellings
    on every view/favorite detail route (never a served response)."""
    slug = tenant_a["workspace"]["slug"]
    ws_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="NC-ws"
    )
    pr_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="NC-pr",
    )
    seed.favorite_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], pr_id,
        project_id=project_a,
    )
    for view_id in (ws_id, pr_id):
        segment = _spellings(view_id)[spelling]
        for _name, method, url in _detail_routes(slug, project_a, segment):
            res = _dispatch(api, method, url, auth_a)
            _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_anonymous_non_canonical_spellings_404(
    seed, tenant_a, project_a, spelling
):
    """Resolver 404 precedes auth: anonymous callers get 404, not 401/403."""
    slug = tenant_a["workspace"]["slug"]
    ws_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="NC-ws-anon"
    )
    segment = _spellings(ws_id)[spelling]
    with httpx.Client(base_url=env.base_url(), timeout=30) as anon:
        for _name, method, url in _detail_routes(slug, project_a, segment):
            res = _dispatch(anon, method, url, {})
            _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_non_canonical_writes_leave_view_unchanged(
    api, seed, db, tenant_a, auth_a, project_a, spelling
):
    """Non-canonical PATCH/PUT are never routed: no write happens."""
    slug = tenant_a["workspace"]["slug"]
    ws_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="NC-ws-kept"
    )
    pr_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="NC-pr-kept",
    )
    for view_id, base in (
        (ws_id, _ws_url(slug, _spellings(ws_id)[spelling])),
        (pr_id, _pr_url(slug, project_a, _spellings(pr_id)[spelling])),
    ):
        for method in ("PATCH", "PUT"):
            res = _dispatch(api, method, base, auth_a)
            _assert_resolver_404(res)
    assert _view_row(db, ws_id) == ("NC-ws-kept", False)
    assert _view_row(db, pr_id) == ("NC-pr-kept", False)


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_non_canonical_delete_leaves_view_live(
    api, seed, db, tenant_a, auth_a, project_a, spelling
):
    """A non-canonical DELETE of a live view is never routed: no write."""
    slug = tenant_a["workspace"]["slug"]
    ws_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], name="NC-ws-live"
    )
    pr_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="NC-pr-live",
    )
    res = api.delete(_ws_url(slug, _spellings(ws_id)[spelling]), headers=auth_a)
    _assert_resolver_404(res)
    res = api.delete(
        _pr_url(slug, project_a, _spellings(pr_id)[spelling]), headers=auth_a
    )
    _assert_resolver_404(res)
    assert _view_row(db, ws_id) == ("NC-ws-live", False)
    assert _view_row(db, pr_id) == ("NC-pr-live", False)


@pytest.mark.parametrize("spelling", ["uppercase", "simple"])
def test_non_canonical_favorite_destroy_leaves_row(
    api, seed, db, tenant_a, auth_a, project_a, spelling
):
    """A non-canonical favorite DELETE is never routed: the row survives."""
    slug = tenant_a["workspace"]["slug"]
    pr_id = seed.issue_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"],
        project_id=project_a, name="NC-pr-fav",
    )
    fav_id = seed.favorite_view(
        tenant_a["workspace"]["id"], tenant_a["user"]["id"], pr_id,
        project_id=project_a,
    )
    res = api.delete(
        _fav_url(slug, project_a, _spellings(pr_id)[spelling]), headers=auth_a
    )
    _assert_resolver_404(res)
    assert _favorite_live(db, fav_id), "favorite row must survive"
