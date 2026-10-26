"""Outsider gate ordering for the intake-issue detail routes (PIDASHCONV-1433).

Django runs `@allow_permission` before the view body parses anything or
looks anything up (`app/views/intake/base.py:328,502,549` over
`app/permissions/base.py:19-84`), so a caller with no membership — or a
non-admin non-creator on PATCH/DELETE — always gets 403
`{"error": "You don't have the required permissions."}`, even for a random
pk, malformed JSON, or a project with no intake row. The Rust handlers
must gate the same way instead of answering 404/400/500 (which would tell
an outsider whether an id exists).
"""

import uuid

import pytest

from _harness.seed import ADMIN
from .conftest import intake_urls

pytestmark = pytest.mark.contract

FORBIDDEN = {"error": "You don't have the required permissions."}

RANDOM_PK = "22222222-2222-4222-8222-222222222222"


def _bare_project_url(owner):
    """A project in the owner's workspace with no intake row."""
    project = owner["seeder"].create_project(owner["workspace"]["id"])
    return (
        f"/api/workspaces/{owner['workspace']['slug']}"
        f"/projects/{project['id']}/intake-issues/"
    )


def test_outsider_patch_random_pk_is_403(admin, make_tenant):
    """PATCH intake-issues/<random pk>/ as an outsider -> 403, not 404."""
    other = make_tenant(role=ADMIN)
    urls = intake_urls(admin)
    r = other["client"].patch(
        f"{urls['intake_issues']}{RANDOM_PK}/", json={"name": "probe"}
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN
    # The inbox-issues alias shares the handler, so it gates the same way.
    r = other["client"].patch(
        f"{urls['inbox_issues']}{RANDOM_PK}/", json={"name": "probe"}
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_outsider_delete_random_pk_is_403(admin, make_tenant):
    """DELETE intake-issues/<random pk>/ as an outsider -> 403, not 404."""
    other = make_tenant(role=ADMIN)
    urls = intake_urls(admin)
    r = other["client"].delete(f"{urls['intake_issues']}{RANDOM_PK}/")
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_outsider_patch_malformed_json_is_403(admin, make_tenant):
    """PATCH with malformed JSON as an outsider -> 403, not 400.

    DRF parses the body lazily inside the view, after the decorator, so
    the permission denial wins over the parse error.
    """
    other = make_tenant(role=ADMIN)
    urls = intake_urls(admin)
    r = other["client"].patch(
        f"{urls['intake_issues']}{RANDOM_PK}/",
        content="{bad json",
        headers={"Content-Type": "application/json"},
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_outsider_get_no_intake_project_is_403(admin, make_tenant):
    """GET on a project with no intake row as an outsider -> 403, not 500."""
    other = make_tenant(role=ADMIN)
    url = _bare_project_url(admin)
    r = other["client"].get(f"{url}{RANDOM_PK}/")
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_outsider_get_random_pk_is_403(admin, make_tenant):
    """GET intake-issues/<random pk>/ as an outsider -> 403, not 404."""
    other = make_tenant(role=ADMIN)
    urls = intake_urls(admin)
    r = other["client"].get(f"{urls['intake_issues']}{RANDOM_PK}/")
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN
