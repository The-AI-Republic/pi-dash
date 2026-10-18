"""Non-canonical ``<uuid:>`` spellings on v1_assets detail paths.

Django routes these URL segments through ``<uuid:...>``, whose converter
matches only lowercase, hyphenated, 36-character UUIDs — anything else is a
resolver 404 that runs *before* authentication and the view. Rust parses the
segment with ``Uuid::parse``, which also accepts uppercase, simple (no
hyphens), braced and ``urn:uuid:`` spellings, so without a spelling gate the
handlers serve — and PATCH/DELETE write — rows Django refuses to route.

Fix (PIDASHCONV-1162, mirroring PIDASHCONV-1095 / -1132): classify the raw
segment before auth. Canonical serves; a parseable-but-non-canonical spelling
proxies to Django so its resolver 404 answers byte for byte; garbage keeps
the local prod resolver body. The resolver 404 is Django's own bytes (the
DEBUG technical page on the harness), so non-canonical spellings answer
``text/html`` on both backends.

Suite: ``pytest api_assets_stickies_intake`` with ``BASE_URL`` / ``DATABASE_URL``
/ ``SECRET_KEY`` in the environment.
"""

import pytest

USER = "/api/v1/assets/user-assets/"
SERVER = "/api/v1/assets/user-assets/server/"
UPLOAD = {
    "name": "profile.jpg",
    "type": "image/jpeg",
    "size": 1024000,
    "entity_type": "USER_AVATAR",
}

# ``(kind, method)`` for every detail path the issue names.
DETAIL_METHODS = [
    ("user", "PATCH"),
    ("user", "DELETE"),
    ("server", "PATCH"),
    ("server", "DELETE"),
    ("generic", "GET"),
    ("generic", "PATCH"),
    ("intake", "GET"),
    ("intake", "PATCH"),
    ("intake", "DELETE"),
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


def _ctype(response):
    return response.headers.get("content-type", "").split(";")[0]


def _assert_resolver_404(response):
    # Resolving the URL precedes auth and the view: Django answers with its
    # own resolver 404 (the HTML technical page under DEBUG), never a 401,
    # a 403, a view 404 JSON body, or 200/204.
    assert response.status_code == 404, response.text
    assert _ctype(response) == "text/html", response.headers.get("content-type")


def _generic_base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/assets/"


def _intake_base(org):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )


def _user_asset(org, base=USER):
    r = org.request("POST", base, "admin", json=UPLOAD)
    assert r.status_code == 200, r.text[:500]
    return r.json()["asset_id"]


def _generic_asset(org):
    r = org.request(
        "POST",
        _generic_base(org),
        "admin",
        json={"name": "report.pdf", "type": "application/pdf", "size": 2048},
    )
    assert r.status_code == 200, r.text[:500]
    return r.json()["asset_id"]


def _provision(org, kind):
    if kind == "user":
        return _user_asset(org)
    if kind == "server":
        return _user_asset(org, base=SERVER)
    if kind == "generic":
        return _generic_asset(org)
    if kind == "intake":
        return org.create_intake_issue(name="non-canonical")["issue"]
    raise AssertionError(kind)


def _detail_url(org, kind, segment):
    if kind == "user":
        return f"{USER}{segment}/"
    if kind == "server":
        return f"{SERVER}{segment}/"
    if kind == "generic":
        return f"{_generic_base(org)}{segment}/"
    if kind == "intake":
        return f"{_intake_base(org)}{segment}/"
    raise AssertionError(kind)


def _state(org, kind, row_id):
    """The row's writable columns, as a stable comparable tuple."""
    if kind in ("user", "server"):
        return org.conn.execute(
            "SELECT is_uploaded, is_deleted, attributes FROM file_assets WHERE id = %s",
            (row_id,),
        ).fetchone()
    if kind == "generic":
        return org.conn.execute(
            "SELECT is_uploaded, is_deleted FROM file_assets WHERE id = %s",
            (row_id,),
        ).fetchone()
    if kind == "intake":
        return org.conn.execute(
            "SELECT deleted_at, name, priority FROM issues WHERE id = %s",
            (row_id,),
        ).fetchone()
    raise AssertionError(kind)


def _body_kw(method):
    return {"json": {}} if method == "PATCH" else {}


@pytest.mark.parametrize("kind,method", DETAIL_METHODS)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_spelling_404s_and_does_not_write(org, kind, method, spelling):
    """Each non-canonical spelling is a resolver 404 and writes nothing.

    PIDASHCONV-1162(a): before the fix Rust parses the segment, reaches auth
    (so an authenticated caller gets a view response) and PATCH/DELETE
    mutates the row; here the row must be byte-identical afterwards.
    """
    row_id = _provision(org, kind)
    before = _state(org, kind, row_id)
    segment = _spellings(row_id)[spelling]
    r = org.request(method, _detail_url(org, kind, segment), "admin", **_body_kw(method))
    _assert_resolver_404(r)
    assert _state(org, kind, row_id) == before, (
        f"{kind} {method} {spelling}: row changed for a segment Django refuses to route"
    )


@pytest.mark.parametrize("kind,method", DETAIL_METHODS)
@pytest.mark.parametrize("spelling", SPELLINGS)
def test_noncanonical_spelling_anonymous_404s(org, kind, method, spelling):
    """The resolver 404 precedes auth: anonymous callers get 404, not 401."""
    row_id = _provision(org, kind)
    segment = _spellings(row_id)[spelling]
    r = org.request(method, _detail_url(org, kind, segment), None, **_body_kw(method))
    _assert_resolver_404(r)


@pytest.mark.parametrize(
    "method,asset_url",
    [
        ("GET", lambda org: f"{_generic_base(org)}not-a-uuid/"),
        ("PATCH", lambda org: f"{_generic_base(org)}not-a-uuid/"),
        ("DELETE", lambda org: f"{USER}not-a-uuid/"),
    ],
)
def test_intake_garbage_segment_body_matches_asset_resolver_404(org, method, asset_url):
    """PIDASHCONV-1162(b): intake's garbage-segment body must be the resolver
    404, not DRF's ``{"detail":"Not found."}``.

    The asset handlers already answer garbage with the prod ``handler404``
    body (``{"error": "Page not found."}`` — plain Django ``JsonResponse``
    bytes, i.e. ``json.dumps`` defaults with the space after the colon);
    intake did not. Compare the two responses: identical status and content
    type, and — when the backend answers JSON (Rust, and Django with
    DEBUG=False) — byte-identical to the prod body. On the harness Django
    (DEBUG=True) both surfaces answer the HTML technical 404, which carries
    the request path and so is compared by content type only.
    """
    kw = _body_kw(method)
    intake = org.request(method, f"{_intake_base(org)}not-a-uuid/", "admin", **kw)
    asset = org.request(method, asset_url(org), "admin", **kw)
    for r in (intake, asset):
        assert r.status_code == 404, r.text
    assert _ctype(intake) == _ctype(asset), (intake.headers, asset.headers)
    if _ctype(intake) == "application/json":
        # The prod resolver-404 bytes, exactly: a parsed-dict comparison
        # would not catch JsonResponse's space after the colon.
        assert intake.content == b'{"error": "Page not found."}', intake.content
        assert asset.content == intake.content, (asset.content, intake.content)
