"""Contract tests: non-canonical UUID path spellings (PIDASHCONV-1197).

Django routes every app_intake detail path param through the
``<uuid:...>`` converter (``pi_dash/app/urls/intake.py``): lowercase hex
and hyphens (``[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}``,
``django/urls/converters.py``), matched case-sensitively. UPPERCASE,
simple (32 hex, no hyphens), ``{braced}`` and ``urn:uuid:...`` spellings,
and garbage segments, therefore never resolve: the URL resolver answers
404 *before* the view -- and therefore before auth -- so an anonymous
caller sees the 404 too.

Rust's ``parse_pk`` / ``parse_id`` used a bare ``Uuid::parse_str`` that
accepts all four non-canonical spellings: the request reached auth and
then the view (200/204, or the view's own 404/500 for a missing row). The
fix gates the raw segment on the canonical spelling ahead of ``actor()``
and answers ``Denial::PageNotFound``.

Note on the asserted 404 body: the contract harness serves Django with
``pi_dash.settings.test`` (``DEBUG = True``), so Django answers a resolver
miss with its technical HTML 404 page, not the production ``handler404``
body ``{"error": "Page not found."}``. Rust, having no Django resolver,
emits that production JSON body directly. These tests therefore pin the
resolver-level contract both backends share -- status 404, and the
handler404 JSON body when the backend emits JSON (Rust / Django under
``DEBUG = False``) or the DEBUG technical page when it serves HTML
(Django under the harness). Before the fix the Rust backend answers
401/403/200/204 instead, so the suite is red on ``rust-dev``.
"""

import uuid

import pytest

from _harness.http import anonymous_client
from _harness.seed import MEMBER
from .conftest import intake_urls
from .test_intake import create_intake_issue
from .test_intake_issue_detail import _teammate, _wait_for_versions

pytestmark = pytest.mark.contract

# Every spelling ``uuid.UUID()`` (and Rust's ``Uuid::parse_str``) accepts
# but Django's case-sensitive ``<uuid:...>`` converter does not route,
# plus a segment that is not a UUID at all.
SPELLINGS = ["uppercase", "simple", "braced", "urn", "garbage"]

# One (route, method) per line the issue names: the intakes / inboxes /
# intake-issues / inbox-issues detail methods, and the two
# description-versions routes. ``versions_detail`` spells the version
# ``pk``; ``versions_detail_work_item`` spells the ``work_item_id``
# segment of the same route (covered for the list route by
# ``versions_list``).
ROUTE_CASES = [
    ("intakes", "GET"),
    ("intakes", "PATCH"),
    ("intakes", "DELETE"),
    ("inboxes", "GET"),
    ("inboxes", "PATCH"),
    ("inboxes", "DELETE"),
    ("intake_issues", "GET"),
    ("intake_issues", "PATCH"),
    ("intake_issues", "DELETE"),
    ("inbox_issues", "GET"),
    ("inbox_issues", "PATCH"),
    ("inbox_issues", "DELETE"),
    ("versions_list", "GET"),
    ("versions_detail", "GET"),
    ("versions_detail_work_item", "GET"),
]


def _spell(canonical, spelling):
    if spelling == "uppercase":
        return canonical.upper()
    if spelling == "simple":
        return canonical.replace("-", "")
    if spelling == "braced":
        return "{" + canonical + "}"
    if spelling == "urn":
        return "urn:uuid:" + canonical
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


@pytest.fixture(scope="session")
def targets(admin, settings):
    """Real rows plus a member and an anonymous client, built once."""
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Spelling probe"
    )
    issue_id = created["issue"]["id"]
    listed = _wait_for_versions(admin["client"], urls["versions"](issue_id))
    version_id = listed["results"][0]["id"]
    member = _teammate(admin, settings, role=MEMBER)
    anon = anonymous_client(settings.base_url)
    return {
        "admin": admin,
        "urls": urls,
        "member": member,
        "anon": anon,
        "intake": admin["intake"]["id"],
        "issue": issue_id,
        "version": version_id,
    }


def _client(targets, actor):
    return targets["anon"] if actor == "anonymous" else targets["member"]


def _disposable_intake(targets):
    admin = targets["admin"]
    return admin["seeder"].create_intake(
        admin["workspace"]["id"],
        admin["project"]["id"],
        name=f"Spelling disposable {uuid.uuid4().hex[:10]}",
    )["id"]


def _disposable_issue(targets):
    created = create_intake_issue(
        targets["admin"]["client"], targets["urls"]["intake_issues"],
        name=f"Spelling disposable issue {uuid.uuid4().hex[:10]}",
    )
    return created["issue"]["id"]


def _spelled_segment(targets, kind, method):
    """The canonical id to spell for this route; a disposable row for the
    mutating methods so a pre-fix request can never damage the shared
    fixtures (the bug lets an uppercased DELETE through)."""
    if kind in ("intakes", "inboxes"):
        if method == "GET":
            return targets["intake"]
        return _disposable_intake(targets)
    if kind in ("intake_issues", "inbox_issues"):
        if method == "GET":
            return targets["issue"]
        return _disposable_issue(targets)
    if kind in ("versions_list", "versions_detail_work_item"):
        return targets["issue"]
    if kind == "versions_detail":
        return targets["version"]
    raise AssertionError(kind)


def _url(targets, kind, method, actor, spelling):
    segment = _spell(_spelled_segment(targets, kind, method), spelling)
    urls = targets["urls"]
    if kind == "intakes":
        return f"{urls['intakes']}{segment}/"
    if kind == "inboxes":
        return f"{urls['inboxes']}{segment}/"
    if kind == "intake_issues":
        return f"{urls['intake_issues']}{segment}/"
    if kind == "inbox_issues":
        return f"{urls['inbox_issues']}{segment}/"
    if kind == "versions_list":
        return urls["versions"](segment)
    if kind == "versions_detail":
        # pk segment spelled, work_item_id canonical.
        return f"{urls['versions'](targets['issue'])}{segment}/"
    if kind == "versions_detail_work_item":
        # work_item_id segment spelled, pk canonical.
        return f"{urls['versions'](segment)}{targets['version']}/"
    raise AssertionError(kind)


def _send(client, method, url):
    if method == "GET":
        return client.get(url)
    if method == "PATCH":
        return client.patch(url, json={"name": "Spelling probe"})
    if method == "DELETE":
        return client.delete(url)
    raise AssertionError(method)


@pytest.mark.parametrize("kind,method", ROUTE_CASES)
@pytest.mark.parametrize("spelling", SPELLINGS)
@pytest.mark.parametrize("actor", ["anonymous", "member"])
def test_non_canonical_uuid_spelling_resolver_404s(
    targets, kind, method, spelling, actor
):
    url = _url(targets, kind, method, actor, spelling)
    response = _send(_client(targets, actor), method, url)
    _assert_resolver_404(response)


@pytest.mark.parametrize(
    "kind,method", [case for case in ROUTE_CASES if case[1] == "GET"]
)
def test_canonical_spelling_still_routes(targets, kind, method):
    """Control: the gate must not reject the one spelling Django routes."""
    segment = _spelled_segment(targets, kind, method)
    urls = targets["urls"]
    if kind == "intakes":
        url = f"{urls['intakes']}{segment}/"
    elif kind == "inboxes":
        url = f"{urls['inboxes']}{segment}/"
    elif kind == "intake_issues":
        url = f"{urls['intake_issues']}{segment}/"
    elif kind == "inbox_issues":
        url = f"{urls['inbox_issues']}{segment}/"
    elif kind == "versions_list":
        url = urls["versions"](segment)
    elif kind == "versions_detail":
        url = f"{urls['versions'](targets['issue'])}{segment}/"
    elif kind == "versions_detail_work_item":
        url = f"{urls['versions'](segment)}{targets['version']}/"
    response = targets["admin"]["client"].get(url)
    assert response.status_code == 200, f"{kind}: {response.status_code} {response.text[:200]}"
