"""Identifier-form `project_id` on the progress/analytics cycle routes.

`_rewrite_project_kwarg` (`app/views/base.py:49-82`) runs before the
permission gate and resolves a non-UUID `project_id` through
`Project.resolve` (`db/models/project.py:192-219`): a matching identifier
becomes the project UUID and the request is served; a miss is a 404
`{"detail": "Project not found"}`. The other cycle routes already do
this; these two used to reject every non-UUID ref with a 400
`{"error": "Please provide valid detail"}`.
"""

from datetime import datetime, timedelta, timezone

import httpx
import pytest

from _harness import env
from _harness.seed import ADMIN
from .conftest import create_cycle_for

pytestmark = pytest.mark.contract

PROJECT_NOT_FOUND = {"detail": "Project not found"}
FORBIDDEN = {"error": "You don't have the required permissions."}
UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}


def _url(tenant: dict, ref: str, cycle_id: str, suffix: str) -> str:
    slug = tenant["workspace"]["slug"]
    return (
        f"/api/workspaces/{slug}/projects/{ref}/cycles/{cycle_id}/{suffix}"
    )


def _past_range(days_ago_start=60, days_ago_end=50):
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


def _dated_cycle(tenant: dict) -> str:
    """Seed a cycle with start/end dates (analytics needs them)."""
    start, end = _past_range()
    return create_cycle_for(
        tenant, name="Ref analytics", start_date=start, end_date=end
    )["id"]


def test_progress_accepts_identifier_project_id(make_tenant):
    tenant = make_tenant()
    ident = tenant["project"]["identifier"]
    pk = tenant["cycle"]["id"]
    by_ident = tenant["client"].get(_url(tenant, ident, pk, "progress/"))
    assert by_ident.status_code == 200, by_ident.text[:500]
    # Identifier form matches the UUID form byte for byte.
    by_uuid = tenant["client"].get(_url(tenant, tenant["project"]["id"], pk, "progress/"))
    assert by_uuid.status_code == 200, by_uuid.text[:500]
    assert by_ident.content == by_uuid.content


def test_progress_accepts_lowercase_identifier(make_tenant):
    tenant = make_tenant()
    lower = tenant["project"]["identifier"].lower()
    r = tenant["client"].get(
        _url(tenant, lower, tenant["cycle"]["id"], "progress/")
    )
    assert r.status_code == 200, r.text[:500]


def test_progress_garbage_project_id_404(make_tenant):
    tenant = make_tenant()
    r = tenant["client"].get(
        _url(tenant, "nope", tenant["cycle"]["id"], "progress/")
    )
    assert r.status_code == 404, r.text[:500]
    assert r.json() == PROJECT_NOT_FOUND


def test_progress_unknown_identifier_404(make_tenant):
    tenant = make_tenant()
    r = tenant["client"].get(
        _url(tenant, "CT1186NOSUCHREF", tenant["cycle"]["id"], "progress/")
    )
    assert r.status_code == 404, r.text[:500]
    assert r.json() == PROJECT_NOT_FOUND


def test_analytics_accepts_identifier_project_id(make_tenant):
    tenant = make_tenant()
    pk = _dated_cycle(tenant)
    ident = tenant["project"]["identifier"]
    by_ident = tenant["client"].get(_url(tenant, ident, pk, "analytics/"))
    assert by_ident.status_code == 200, by_ident.text[:500]
    by_uuid = tenant["client"].get(
        _url(tenant, tenant["project"]["id"], pk, "analytics/")
    )
    assert by_uuid.status_code == 200, by_uuid.text[:500]
    assert by_ident.content == by_uuid.content


def test_analytics_accepts_lowercase_identifier(make_tenant):
    tenant = make_tenant()
    pk = _dated_cycle(tenant)
    lower = tenant["project"]["identifier"].lower()
    r = tenant["client"].get(_url(tenant, lower, pk, "analytics/"))
    assert r.status_code == 200, r.text[:500]


def test_analytics_garbage_project_id_404(make_tenant):
    tenant = make_tenant()
    r = tenant["client"].get(
        _url(tenant, "nope", tenant["cycle"]["id"], "analytics/")
    )
    assert r.status_code == 404, r.text[:500]
    assert r.json() == PROJECT_NOT_FOUND


def test_identifier_resolution_precedes_permission_gate(admin, make_tenant):
    """`_rewrite_project_kwarg` runs before the permission gate.

    An authenticated outsider: a real identifier resolves and then the
    gate answers 403; a garbage ref never reaches the gate — it is a 404
    from the resolve step.
    """
    outsider = make_tenant(role=ADMIN)
    pk = admin["cycle"]["id"]
    r = outsider["client"].get(_url(admin, admin["project"]["identifier"], pk, "progress/"))
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN
    r = outsider["client"].get(_url(admin, "nope", pk, "progress/"))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == PROJECT_NOT_FOUND


def test_identifier_anonymous_401(make_tenant):
    """Anonymous callers skip resolution and 401 before anything else."""
    tenant = make_tenant()
    anon = httpx.Client(base_url=env.base_url(), timeout=30)
    try:
        r = anon.get(
            _url(tenant, tenant["project"]["identifier"], tenant["cycle"]["id"], "progress/")
        )
    finally:
        anon.close()
    assert r.status_code == 401, r.text[:500]
    assert r.json() == UNAUTHENTICATED
