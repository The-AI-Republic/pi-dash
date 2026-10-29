"""Non-canonical ``<uuid:>`` spellings on app_assets routes (PIDASHCONV-1399).

Django declares every segment below with ``<uuid:...>``
(``apps/api/pi_dash/app/urls/asset.py``), whose converter matches only the
canonical lowercase hyphenated form — every other spelling resolver-404s
before authentication and the view. Rust parsed the segment with
``Uuid::parse``, which also accepts uppercase, simple (no hyphens) and
braced spellings, so it served (and PATCH/DELETE/POST wrote through)
URLs Django never routes.

Fix: gate each path segment on ``classify_uuid_segment`` before auth;
NonCanonical answers the same local resolver-404 body the garbage path
already serves (``page_not_found()``), since no app_assets handler carries
proxy plumbing and the prod custom ``handler404`` bytes
(``pi_dash/urls.py:15`` -> ``error_404.py``) are pinned by
PIDASHCONV-1215. Body-field UUID parsing is not a path converter and is
unchanged.

The resolver 404 is ``b'{"error": "Page not found."}'`` where the backend
answers JSON (Rust always; Django with ``DEBUG = False``). The harness
serves Django with ``pi_dash.settings.test`` (``DEBUG = True``), so on
Django these assertions see the technical HTML page instead — the same
split as ``test_resolver_404_bytes.py`` (PIDASHCONV-1215).
"""

import pytest

from . import seed_assets as seed_a

pytestmark = pytest.mark.contract

RESOLVER_404 = b'{"error": "Page not found."}'

# ``(kind, method)`` for every route with a ``<uuid:>`` segment.
DETAIL_METHODS = [
    ("v1ws", "GET"),
    ("v1ws", "DELETE"),
    ("v1restore", "POST"),
    ("user", "PATCH"),
    ("user", "DELETE"),
    ("ws", "GET"),
    ("ws", "PATCH"),
    ("ws", "DELETE"),
    ("static", "GET"),
    ("restore", "POST"),
    ("proj", "GET"),
    ("proj", "PATCH"),
    ("proj", "DELETE"),
    ("bulk", "POST"),
    ("check", "GET"),
    ("dup", "POST"),
    ("wsdl", "GET"),
    ("projdl", "GET"),
]

SPELLINGS = ["uppercase", "simple", "braced"]


def _spellings(row_id):
    """Spellings ``Uuid::parse`` accepts but Django's ``<uuid:>`` does not."""
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
        "braced": "{" + row_id + "}",
    }


def _assert_resolver_404(response):
    # Resolution precedes auth and the view: never a 401, a 403, a view
    # body, or 200/204/302. JSON backends must serve the prod handler404
    # bytes exactly (a parsed-dict comparison would miss the spacing);
    # harness Django (DEBUG=True) serves the technical HTML page.
    assert response.status_code == 404, response.text
    ctype = response.headers.get("content-type", "")
    if ctype.startswith("application/json"):
        assert response.content == RESOLVER_404, response.content
    else:
        assert "text/html" in ctype, ctype


def _assert_not_resolver_404(response):
    # Canonical control: the spelling Django routes must reach a view.
    if response.status_code == 404:
        ctype = response.headers.get("content-type", "")
        assert not (
            response.content == RESOLVER_404 or "text/html" in ctype
        ), response.content[:200]


def _mint_ws(org, **kw):
    payload = {
        "name": "logo.png",
        "type": "image/png",
        "size": 100,
        "entity_type": "WORKSPACE_LOGO",
        "entity_identifier": org.workspace["id"],
    }
    payload.update(kw)
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/",
        "admin", json=payload,
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def _mint_user(org):
    r = org.request(
        "POST", "/api/assets/v2/user-assets/", "admin",
        json={"name": "avatar.png", "type": "image/png", "size": 512,
              "entity_type": "USER_AVATAR"},
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def _mint_proj(org, **kw):
    payload = {
        "name": "desc.png",
        "type": "image/png",
        "size": 200,
        "entity_type": "ISSUE_DESCRIPTION",
        "entity_identifier": None,
    }
    payload.update(kw)
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/",
        "admin", json=payload,
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def _proj_base(org):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/"
    )


class Provision:
    """One route kind: the canonical id to respell, its URL builder, and
    the write guard (None for read-only routes)."""

    def __init__(self, seg, url, guard=None):
        self.seg = seg
        self.url = url
        self.guard = guard


def _provision(org, kind):
    slug = org.workspace["slug"]
    if kind == "v1ws":
        row = seed_a.create_asset(
            org.conn, workspace_id=org.workspace["id"],
            created_by_id=org.admin["id"], name="nc.txt",
            entity_type="ISSUE_ATTACHMENT", uploaded=True,
        )
        leaf = row["key"].split("/", 1)[1]
        aid = row["id"]
        return Provision(
            org.workspace["id"],
            lambda seg: f"/api/workspaces/file-assets/{seg}/{leaf}/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "v1restore":
        row = seed_a.create_asset(
            org.conn, workspace_id=org.workspace["id"],
            created_by_id=org.admin["id"], name="ncr.txt",
            entity_type="ISSUE_ATTACHMENT", uploaded=True,
        )
        leaf = row["key"].split("/", 1)[1]
        aid = row["id"]
        return Provision(
            org.workspace["id"],
            lambda seg: f"/api/workspaces/file-assets/{seg}/{leaf}/restore/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "user":
        aid = _mint_user(org)
        return Provision(
            aid, lambda seg: f"/api/assets/v2/user-assets/{seg}/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "ws":
        aid = _mint_ws(org)
        return Provision(
            aid,
            lambda seg: f"/api/assets/v2/workspaces/{slug}/{seg}/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "static":
        aid = _mint_ws(org)
        seed_a.mark_uploaded(org.conn, aid)
        return Provision(aid, lambda seg: f"/api/assets/v2/static/{seg}/")
    if kind == "restore":
        aid = _mint_ws(org)
        return Provision(
            aid,
            lambda seg: f"/api/assets/v2/workspaces/{slug}/restore/{seg}/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "proj":
        aid = _mint_proj(org)
        return Provision(
            aid, lambda seg: f"{_proj_base(org)}{seg}/",
            guard=lambda: seed_a.fetch(org.conn, aid),
        )
    if kind == "bulk":
        first = _mint_proj(org, name="b1.png")
        second = _mint_proj(org, name="b2.png")
        import uuid as _uuid

        entity_id = str(_uuid.uuid4())
        return Provision(
            entity_id, lambda seg: f"{_proj_base(org)}{seg}/bulk/",
            guard=lambda: (
                seed_a.fetch(org.conn, first),
                seed_a.fetch(org.conn, second),
            ),
        )
    if kind == "check":
        aid = _mint_ws(org)
        return Provision(
            aid,
            lambda seg: f"/api/assets/v2/workspaces/{slug}/check/{seg}/",
        )
    if kind == "dup":
        aid = _mint_ws(org)
        seed_a.mark_uploaded(org.conn, aid)
        return Provision(
            aid,
            lambda seg: (
                f"/api/assets/v2/workspaces/{slug}/duplicate-assets/{seg}/"
            ),
            guard=lambda: (
                seed_a.fetch(org.conn, aid),
                _asset_count(org),
            ),
        )
    if kind == "wsdl":
        aid = _mint_ws(org)
        seed_a.mark_uploaded(org.conn, aid)
        return Provision(
            aid,
            lambda seg: f"/api/assets/v2/workspaces/{slug}/download/{seg}/",
        )
    if kind == "projdl":
        aid = _mint_proj(org)
        seed_a.mark_uploaded(org.conn, aid)
        return Provision(
            aid, lambda seg: f"{_proj_base(org)}download/{seg}/",
        )
    raise AssertionError(kind)


def _asset_count(org):
    return org.conn.execute("SELECT count(*) FROM file_assets").fetchone()[0]


def _body_kw(org, kind, method):
    if method == "PATCH":
        return {"json": {}}
    if method == "POST":
        if kind == "dup":
            return {"json": {"entity_type": "WORKSPACE_LOGO",
                             "entity_id": org.workspace["id"]}}
        if kind == "bulk":
            return {"json": {"asset_ids": []}}
        return {}
    return {}


@pytest.mark.parametrize("kind,method", DETAIL_METHODS)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_spelling_404s_and_does_not_write(
    org, kind, method, spelling
):
    """Each non-canonical spelling resolver-404s and writes nothing.

    Before the fix Rust parses the segment and serves it (302/200 on
    GET, row writes on PATCH/DELETE/POST); the guarded row must be
    byte-identical afterwards.
    """
    prov = _provision(org, kind)
    before = prov.guard() if prov.guard else None
    segment = _spellings(prov.seg)[spelling]
    r = org.request(
        method, prov.url(segment), "admin", **_body_kw(org, kind, method)
    )
    _assert_resolver_404(r)
    if before is not None:
        assert prov.guard() == before, (
            f"{kind} {method} {spelling}: row changed for a segment "
            "Django refuses to route"
        )


@pytest.mark.parametrize("kind,method", DETAIL_METHODS)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_spelling_anonymous_404s(org, kind, method, spelling):
    """The resolver 404 precedes auth: anonymous callers get 404, not 401."""
    prov = _provision(org, kind)
    segment = _spellings(prov.seg)[spelling]
    r = org.request(
        method, prov.url(segment), None, **_body_kw(org, kind, method)
    )
    _assert_resolver_404(r)


@pytest.mark.parametrize("kind,method", DETAIL_METHODS)
def test_canonical_spelling_still_routes(org, kind, method):
    """Control: the canonical spelling reaches a view, never the resolver."""
    prov = _provision(org, kind)
    r = org.request(
        method, prov.url(prov.seg), "admin", **_body_kw(org, kind, method)
    )
    _assert_not_resolver_404(r)
