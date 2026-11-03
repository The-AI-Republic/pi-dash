"""Outsider gate ordering for the intake-issue detail routes (PIDASHCONV-1433).

Django runs `@allow_permission` before the view body parses anything or
looks anything up (`app/views/intake/base.py:328,502,549` over
`app/permissions/base.py:19-84`), so a caller with no membership — or a
non-admin non-creator on PATCH/DELETE — always gets 403
`{"error": "You don't have the required permissions."}`, even for a random
pk, malformed JSON, or a project with no intake row. The Rust handlers
must gate the same way instead of answering 404/400/500 (which would tell
an outsider whether an id exists).

The last test pins the post-gate raise order instead: a gate-passing
caller (the issue creator) with a bogus project UUID must get Django's
`Project.objects.get` 404, not the missing-intake 500.
"""

import uuid

import pytest

from _harness.seed import ADMIN
from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

FORBIDDEN = {"error": "You don't have the required permissions."}
NOT_FOUND = {"error": "The required object does not exist."}

RANDOM_PK = "22222222-2222-4222-8222-222222222222"
BOGUS_PROJECT = "33333333-3333-4333-8333-333333333333"


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


def test_creator_get_bogus_project_is_404(make_tenant):
    """GET with a bogus project UUID as the issue creator -> 404, not 500.

    Both creator-branch checks are project-independent (workspace
    membership by slug, `Issue.created_by` by pk), so the gate passes;
    Django's retrieve body then raises `Project.objects.get` before it
    dereferences the missing intake (`filter().first()` never raises),
    answering 404. The Rust port must raise in the same order.
    """
    owner = make_tenant(role=ADMIN)
    created = create_intake_issue(
        owner["client"], intake_urls(owner)["intake_issues"]
    )
    issue_id = created["issue"]["id"]
    # Pin the creator precondition on both backends, regardless of how
    # the create path stamps `Issue.created_by`.
    owner["seeder"].db.execute(
        "UPDATE issues SET created_by_id = %s WHERE id = %s",
        (owner["user"]["id"], issue_id),
    )
    url = (
        f"/api/workspaces/{owner['workspace']['slug']}"
        f"/projects/{BOGUS_PROJECT}/intake-issues/{issue_id}/"
    )
    r = owner["client"].get(url)
    assert r.status_code == 404, r.text[:500]
    assert r.json() == NOT_FOUND
