"""Non-canonical UUID spellings resolver-404 (PIDASHCONV-1456).

Django routes both detail paths through the ``<uuid:...>`` converter
(``pi_dash/app/urls/analytic.py:36`` ``analytic-view/<uuid:pk>/`` and
``:41`` ``saved-analytic-view/<uuid:analytic_id>/``), whose regex
(``django/urls/converters.py``) accepts only lowercase, hyphenated
UUIDs, matched case-sensitively. Every other spelling never resolves:
the URL resolver answers 404 *before* the view -- and therefore before
auth -- so an anonymous caller sees the 404 too.

Rust parsed the segment with ``Uuid::parse_str``, which also accepts
uppercase, simple (no hyphens), braced and ``urn:uuid:`` forms: the
non-canonical spellings were served (200, and PATCH/DELETE mutated the
row), while garbage answered 400 ``{"error": "Please provide valid
detail"}`` after the auth gate. The fix gates the raw segment on the
canonical spelling before auth and proxies every other spelling to
Django for its own resolver 404.

Each test below fails on the pre-fix Rust, passes after the fix, and
passes on Django. The mutating methods use disposable rows so a pre-fix
request can never damage the shared fixtures (the bug lets an
uppercased PATCH/DELETE through); the row snapshot pins that the view
never runs.
"""

import json
import uuid
from datetime import datetime, timezone

import pytest

from _harness.http import anonymous_client

NOW = datetime.now(timezone.utc)

SPELLINGS = ["uppercase", "simple", "garbage"]


def _spell(canonical, spelling):
    if spelling == "uppercase":
        return canonical.upper()
    if spelling == "simple":
        return canonical.replace("-", "")
    if spelling == "garbage":
        return "not-a-uuid"
    raise AssertionError(f"unknown spelling {spelling!r}")


def _assert_resolver_404(response):
    assert response.status_code == 404, (
        f"expected resolver 404, got {response.status_code}: {response.text[:200]!r}"
    )
    content_type = response.headers.get("content-type", "")
    if content_type.startswith("application/json"):
        assert response.json() == {"error": "Page not found."}, response.text
    else:
        assert content_type.startswith("text/html"), content_type


def _insert_view(db_conn, seed, name):
    view_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO analytic_views (created_at, updated_at, id, name,"
            " description, query, query_dict, created_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, view_id, name, "1456 probe",
             json.dumps({"workspace__slug": seed["ws_slug"]}),
             json.dumps({"x_axis": "priority", "y_axis": "issue_count"}),
             seed["admin"], seed["ws"]),
        )
    return view_id


def _delete_view(db_conn, view_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM analytic_views WHERE id = %s", (view_id,))


def _snapshot(db_conn, view_id):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT name, description, query, query_dict, deleted_at, updated_at"
            " FROM analytic_views WHERE id = %s",
            (view_id,),
        )
        return cur.fetchone()


def _client_for(clients, base_url, actor):
    if actor == "anonymous":
        return anonymous_client(base_url)
    return clients["admin"]


@pytest.mark.parametrize("spelling", SPELLINGS)
@pytest.mark.parametrize("method", ["GET", "PATCH", "DELETE"])
@pytest.mark.parametrize("actor", ["admin", "anonymous"])
def test_analytic_view_non_canonical_spelling_resolver_404s(
    clients, base_url, seed, db_conn, spelling, method, actor
):
    """Non-canonical pk spellings 404 on analytic-view detail; row untouched."""
    if method == "GET":
        view_id = seed["view"]
        disposable = None
    else:
        disposable = _insert_view(
            db_conn, seed, f"AV-1456 disposable {uuid.uuid4().hex[:8]}"
        )
        view_id = disposable
    before = _snapshot(db_conn, view_id)
    url = (
        f"/api/workspaces/{seed['ws_slug']}/analytic-view/"
        f"{_spell(view_id, spelling)}/"
    )
    client = _client_for(clients, base_url, actor)
    try:
        if method == "GET":
            response = client.get(url)
        elif method == "PATCH":
            response = client.patch(url, json={"name": "Spelling probe"})
        else:
            response = client.delete(url)
        _assert_resolver_404(response)
        assert _snapshot(db_conn, view_id) == before
    finally:
        if actor == "anonymous":
            client.close()
        if disposable is not None:
            _delete_view(db_conn, disposable)


@pytest.mark.parametrize("spelling", SPELLINGS)
@pytest.mark.parametrize("actor", ["admin", "anonymous"])
def test_saved_analytic_view_non_canonical_spelling_resolver_404s(
    clients, base_url, seed, spelling, actor
):
    """Non-canonical id spellings 404 on saved-analytic-view detail."""
    url = (
        f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/"
        f"{_spell(seed['view'], spelling)}/"
    )
    client = _client_for(clients, base_url, actor)
    try:
        _assert_resolver_404(client.get(url))
    finally:
        if actor == "anonymous":
            client.close()


def test_canonical_spelling_still_routes(clients, seed):
    """Control: the gate must not reject the one spelling Django routes."""
    admin = clients["admin"]
    ws = seed["ws_slug"]
    resp = admin.get(f"/api/workspaces/{ws}/analytic-view/{seed['view']}/")
    assert resp.status_code == 200, f"analytic-view: {resp.status_code} {resp.text[:200]}"
    resp = admin.get(f"/api/workspaces/{ws}/saved-analytic-view/{seed['view']}/")
    assert resp.status_code == 200, (
        f"saved-analytic-view: {resp.status_code} {resp.text[:200]}"
    )
