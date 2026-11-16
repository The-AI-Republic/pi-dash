"""PIDASHCONV-1190: request-body parsing for the two cycle write handlers.

`CycleFavoriteViewSet.create` (`base.py:571-579`) and
`CycleIssueViewSet.create` (`issue.py:223`) read `request.data` with DRF's
default parsers (JSON, form, multipart), after auth and the role gate.
These tests pin the divergence class the `Json<Value>` axum extractor
introduced: an empty body, a urlencoded body, a malformed JSON body and an
unsupported content type each answer with Django's status and JSON body
(and, for favorites, the row Django writes).
"""

import uuid

import pytest

from _harness.seed import MEMBER
from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _favorite_identifiers(tenant):
    """Live `user_favorites.entity_identifier` rows for the tenant."""
    with tenant["conn"].cursor() as cur:
        cur.execute(
            "select entity_identifier from user_favorites "
            "where project_id = %s and deleted_at is null",
            (tenant["project"]["id"],),
        )
        return [row[0] for row in cur.fetchall()]


def test_favorite_create_empty_body(make_tenant):
    # No body / no Content-Type: DRF's `_load_stream` sees Content-Length 0
    # and hands the view `{}`, so `entity_identifier` is NULL and the 204
    # still writes a row.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    r = tenant["client"].post(urls["favorites"])
    assert r.status_code == 204, r.text[:500]
    assert _favorite_identifiers(tenant) == [None]


def test_favorite_create_urlencoded_body(make_tenant):
    # `Content-Type: application/x-www-form-urlencoded` with `cycle=<uuid>`
    # is served: `request.data.get("cycle")` is the UUID string.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    pk = tenant["cycle"]["id"]
    r = tenant["client"].post(
        urls["favorites"],
        content=f"cycle={pk}",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    assert r.status_code == 204, r.text[:500]
    assert _favorite_identifiers(tenant) == [uuid.UUID(pk)]


def test_favorite_create_malformed_json_body(make_tenant):
    # DRF's JSONParser raises ParseError, which renders the CPython message.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["favorites"],
        content=b'{"cycle":',
        headers={"Content-Type": "application/json"},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json()["detail"].startswith("JSON parse error - ")
    assert _favorite_identifiers(tenant) == []


def test_cycle_issue_create_empty_body(make_tenant):
    # Empty body -> `{}` -> `request.data.get("issues", [])` is `[]`.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        content=b"",
        headers={"Content-Type": "application/json"},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Issues are required"}


def test_cycle_issue_create_urlencoded_body(make_tenant):
    # A form body parses to the scalar string, so the `issue_id__in`
    # coercion raises `ValidationError` -> the view's own 400 body.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    issue = create_issue_for(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        content=f"issues={issue['id']}",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Please provide valid detail"}


def test_cycle_issue_create_malformed_json_body(make_tenant):
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        content=b'{"issues":',
        headers={"Content-Type": "application/json"},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json()["detail"].startswith("JSON parse error - ")


def test_write_body_unsupported_media_type(make_tenant):
    # No default parser matches `text/plain`, so DRF answers 415 with the
    # media type echoed back — on both write paths.
    tenant = make_tenant(role=MEMBER)
    urls = cycle_urls(tenant)
    expected = {"detail": 'Unsupported media type "text/plain" in request.'}
    r = tenant["client"].post(
        urls["favorites"], content=b"x=1",
        headers={"Content-Type": "text/plain"},
    )
    assert r.status_code == 415, r.text[:500]
    assert r.json() == expected
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]), content=b"x=1",
        headers={"Content-Type": "text/plain"},
    )
    assert r.status_code == 415, r.text[:500]
    assert r.json() == expected
