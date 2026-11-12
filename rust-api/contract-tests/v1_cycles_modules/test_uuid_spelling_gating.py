"""Contract tests: non-canonical UUID path spellings (PIDASHCONV-1193).

Django routes every UUID path param in this slice through the ``<uuid:...>``
converter (``django/urls/converters.py``): lowercase hex and hyphens, matched
case-sensitively. UPPERCASE, simple (32 hex, no hyphens), ``{braced}`` and
``urn:uuid:...`` spellings therefore never resolve -- the resolver answers 404
*after* routing and *before* auth, so an anonymous caller sees the 404 too.

Rust's ``path_uuid`` used ``Uuid::parse_str``, which accepts all four: the
request reached auth and then the view (200/204, or a view-level 404 for a
missing row). The fix gates the raw segment on the canonical spelling and
proxies everything else to Django for its own resolver 404 -- the
PIDASHCONV-1095 (v1_cli_auth) and PIDASHCONV-1132 (license) precedent.

Note on the asserted 404 body: the contract harness serves Django with
``pi_dash.settings.test`` (``DEBUG = True``), so Django answers a resolver
miss with its technical HTML 404 page, not the production ``handler404``
body ``{"error": "Page not found."}``. These tests pin the former (status
404 + html), because that is the behavior both backends must match under the
harness; the production JSON handler404 only appears with ``DEBUG = False``
and is unreachable here.
"""

import pytest

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID


def _spellings(canonical):
    # Every spelling ``uuid.UUID()`` (and Rust's ``Uuid::parse_str``) accepts
    # but Django's ``<uuid:...>`` converter (``[0-9a-f]{8}-...``, matched
    # case-sensitively) does not route.
    return {
        "uppercase": canonical.upper(),
        "simple": canonical.replace("-", ""),
        "braced": "{" + canonical + "}",
        "urn": "urn:uuid:" + canonical,
    }


def _assert_resolver_404(res):
    # Routing precedes auth and the view entirely: the answer is the
    # resolver's HTML 404, never the view's JSON body or a 401.
    assert res.status_code == 404, res.text
    assert res.headers.get("content-type", "").startswith("text/html"), res.headers.get(
        "content-type"
    )


def _cycle_url(segment):
    return api.cycle_detail_url(SLUG, PROJ, segment)


def _module_url(segment):
    return api.module_detail_url(SLUG, PROJ, segment)


def _nested_url(segment):
    return api.cycle_issues_url(SLUG, PROJ, segment)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_cycle_detail_non_canonical_spelling_authenticated_404s(
    member_client, spelling
):
    segment = _spellings(db.CYCLE_ACTIVE_ID)[spelling]
    res = member_client.get(_cycle_url(segment))
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_cycle_detail_non_canonical_spelling_anonymous_404s(anon_client, spelling):
    segment = _spellings(db.CYCLE_ACTIVE_ID)[spelling]
    res = anon_client.get(_cycle_url(segment))
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_module_detail_non_canonical_spelling_authenticated_404s(
    member_client, spelling
):
    segment = _spellings(db.MODULE_ACTIVE_ID)[spelling]
    res = member_client.get(_module_url(segment))
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_module_detail_non_canonical_spelling_anonymous_404s(anon_client, spelling):
    segment = _spellings(db.MODULE_ACTIVE_ID)[spelling]
    res = anon_client.get(_module_url(segment))
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_nested_cycle_issues_non_canonical_spelling_authenticated_404s(
    member_client, spelling
):
    segment = _spellings(db.CYCLE_ACTIVE_ID)[spelling]
    res = member_client.get(_nested_url(segment))
    _assert_resolver_404(res)


@pytest.mark.parametrize("spelling", ["uppercase", "simple", "braced", "urn"])
def test_nested_cycle_issues_non_canonical_spelling_anonymous_404s(
    anon_client, spelling
):
    segment = _spellings(db.CYCLE_ACTIVE_ID)[spelling]
    res = anon_client.get(_nested_url(segment))
    _assert_resolver_404(res)


def test_canonical_spellings_still_route(member_client):
    # Control: the gate must not reject the one spelling Django routes.
    assert (
        member_client.get(_cycle_url(db.CYCLE_ACTIVE_ID)).status_code == 200
    ), "canonical cycle detail"
    assert (
        member_client.get(_module_url(db.MODULE_ACTIVE_ID)).status_code == 200
    ), "canonical module detail"
    assert (
        member_client.get(_nested_url(db.CYCLE_ACTIVE_ID)).status_code == 200
    ), "canonical cycle-issues"
