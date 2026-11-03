"""Non-canonical ``<uuid:pk>`` spellings on app_notifications per-pk routes (PIDASHCONV-1457).

Django declares the pk segment with ``<uuid:pk>``
(``apps/api/pi_dash/app/urls/notification.py:22,27,32``), whose converter
matches only lowercase, hyphenated, 36-character UUIDs — anything else is a
resolver 404 that runs *before* authentication and the view. Rust parsed the
segment with ``Uuid::parse``, which also accepts uppercase, simple (no
hyphens), braced and ``urn:uuid:`` spellings, so it served — and PATCH /
read / archive / DELETE wrote through — URLs Django never routes.

Fix: gate the raw segment on ``crate::uuid_path::classify_uuid_segment``
before auth. Canonical serves; NonCanonical and Garbage proxy to Django so
its resolver 404 answers byte for byte (the garbage path already proxied;
``unread/`` + ``mark-all-read/`` literals and the 404 body live there).

The resolver 404 is Django's own bytes: the harness serves Django with
``pi_dash.settings.test`` (``DEBUG = True``), so both backends answer the
HTML technical page (the Rust leg proxies to its Django upstream).
"""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from _harness import db  # noqa: E402
from _harness.config import database_url  # noqa: E402

import pytest  # noqa: E402

# (route, method, url builder, request kwargs) for every per-pk route.
CASES = [
    ("detail", "GET", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/", {}),
    ("Detail", "PATCH", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/",
     {"json": {"snoozed_till": "2030-01-01T00:00:00Z"}}),
    ("Detail", "DELETE", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/", {}),
    ("read", "POST", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/read/", {}),
    ("read", "DELETE", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/read/", {}),
    ("archive", "POST", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/archive/", {}),
    ("archive", "DELETE", lambda slug, seg: f"/api/workspaces/{slug}/users/notifications/{seg}/archive/", {}),
]

SPELLINGS = ["uppercase", "simple", "braced", "urn"]


def _spellings(row_id):
    """Spellings ``Uuid::parse`` accepts but Django's ``<uuid:>`` does not."""
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
        "braced": "{" + row_id + "}",
        "urn": "urn:uuid:" + row_id,
    }


def _row(notification_id):
    return db.fetchone(
        database_url(),
        "SELECT read_at, archived_at, snoozed_till, deleted_at, updated_at"
        " FROM notifications WHERE id = %s",
        (notification_id,),
    )


def _assert_resolver_404(response):
    # Resolution precedes auth and the view: never a 401, a 403, a view
    # 404 JSON body, or 200/204. Both backends answer Django's resolver
    # page (Rust proxies to its Django upstream).
    assert response.status_code == 404, response.text[:500]
    ctype = response.headers.get("content-type", "")
    assert "text/html" in ctype, ctype


def _request(client, method, url, kwargs):
    if method == "GET":
        return client.get(url)
    if method == "PATCH":
        return client.patch(url, **kwargs)
    if method == "POST":
        return client.post(url, **kwargs)
    if method == "DELETE":
        return client.delete(url)
    raise AssertionError(method)


@pytest.mark.parametrize("route,method,url,kwargs", CASES)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_pk_spellings_404(ctx, api_owner, mknotif, route, method, url, kwargs, spelling):
    """Each non-canonical spelling resolver-404s and writes nothing.

    Before the fix Rust serves the spelling (200 on GET/read/archive,
    row writes on PATCH/DELETE); the row must be byte-identical
    afterwards.
    """
    n = mknotif(title=f"nc-{route}-{method}-{spelling}")
    before = _row(n["id"])
    segment = _spellings(n["id"])[spelling]
    r = _request(api_owner, method, url(ctx.slug_a, segment), kwargs)
    _assert_resolver_404(r)
    assert _row(n["id"]) == before, (
        f"{route} {method} {spelling}: row changed for a segment Django refuses to route"
    )


@pytest.mark.parametrize("route,method,url,kwargs", CASES)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_pk_spellings_anonymous_404(ctx, api_anon, mknotif, route, method, url, kwargs, spelling):
    """The resolver 404 precedes auth: anonymous callers get 404, not 401."""
    n = mknotif(title=f"nc-anon-{route}-{method}-{spelling}")
    segment = _spellings(n["id"])[spelling]
    r = _request(api_anon, method, url(ctx.slug_a, segment), kwargs)
    _assert_resolver_404(r)


@pytest.mark.parametrize("route,method,url,kwargs", CASES)
def test_canonical_pk_still_routes(ctx, api_owner, mknotif, route, method, url, kwargs):
    """Control: the canonical spelling reaches a view, never the resolver."""
    n = mknotif(title=f"canon-{route}-{method}")
    r = _request(api_owner, method, url(ctx.slug_a, n["id"]), kwargs)
    assert r.status_code != 404 or "text/html" not in r.headers.get("content-type", ""), r.text[:300]
    # And it really hit the view: GET/PATCH/POST/DELETE-read/archive answer
    # 200, DELETE answers 204.
    assert r.status_code in (200, 204), (r.status_code, r.text[:300])
