"""Missing-project / unknown-slug gate ordering for the cycle routes.

Python runs the permission gate before any project (or workspace) row
lookup: `_rewrite_project_kwarg` (`app/views/base.py:49-82`) returns
early for a UUID `project_id` with no row check, and `@allow_permission`
(`app/permissions/base.py:19-80`) then finds no `ProjectMember`, so a
caller with no access to a nonexistent project gets 403 — the view never
runs. Only the identifier form resolves the project first, via
`Project.resolve`, answering 404 {"detail": "Project not found"}.

These cases pin that ordering on date-check, transfer-issues and
user-properties so the Rust handlers cannot answer 404 (which leaks
whether a project id or workspace slug exists) where Django says 403.
"""

import uuid

import pytest

from _harness.seed import ADMIN

pytestmark = pytest.mark.contract

FORBIDDEN = {"error": "You don't have the required permissions."}
PROJECT_NOT_FOUND = {"detail": "Project not found"}


def _cycle_uuid() -> str:
    return str(uuid.uuid4())


def test_random_uuid_project_is_403_on_date_check(make_tenant):
    """POST cycles/date-check/ with a nonexistent project UUID -> 403."""
    tenant = make_tenant(role=ADMIN)
    url = (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{uuid.uuid4()}/cycles/date-check/"
    )
    r = tenant["client"].post(
        url, json={"start_date": "2026-03-01", "end_date": "2026-03-31"}
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_random_uuid_project_is_403_on_transfer(make_tenant):
    """POST transfer-issues/ with a nonexistent project UUID -> 403."""
    tenant = make_tenant(role=ADMIN)
    url = (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{uuid.uuid4()}/cycles/{_cycle_uuid()}/transfer-issues/"
    )
    r = tenant["client"].post(url, json={"new_cycle_id": _cycle_uuid()})
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_random_uuid_project_is_403_on_user_properties(make_tenant):
    """GET and PATCH user-properties/ with a nonexistent project UUID -> 403."""
    tenant = make_tenant(role=ADMIN)
    url = (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{uuid.uuid4()}/cycles/{_cycle_uuid()}/user-properties/"
    )
    r = tenant["client"].get(url)
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN
    r = tenant["client"].patch(url, json={"filters": {}})
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_unknown_slug_uuid_project_is_403_on_user_properties(make_tenant):
    """Unknown workspace slug + real project UUID -> 403 (gate first)."""
    tenant = make_tenant(role=ADMIN)
    url = (
        f"/api/workspaces/no-such-ws-{uuid.uuid4().hex[:8]}"
        f"/projects/{tenant['project']['id']}/cycles/{_cycle_uuid()}"
        f"/user-properties/"
    )
    r = tenant["client"].get(url)
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN
    r = tenant["client"].patch(url, json={"filters": {}})
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_unknown_slug_identifier_project_is_404_detail(make_tenant):
    """Unknown workspace slug + project identifier -> 404 Project not found."""
    tenant = make_tenant(role=ADMIN)
    with tenant["conn"].cursor() as cur:
        cur.execute(
            "select identifier from projects where id = %s",
            (tenant["project"]["id"],),
        )
        identifier = cur.fetchone()[0]
    url = (
        f"/api/workspaces/no-such-ws-{uuid.uuid4().hex[:8]}"
        f"/projects/{identifier}/cycles/{_cycle_uuid()}/user-properties/"
    )
    r = tenant["client"].get(url)
    assert r.status_code == 404, r.text[:500]
    assert r.json() == PROJECT_NOT_FOUND
