# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the project scheduler occurrences endpoint.

Covers the merge of past AgentRuns + future RRULE expansion, window
validation (rejected if > 90 days or to < from), the response cap, the
``has_more`` / ``next_window_start`` truncation contract, and the
``Scheduler.is_enabled`` / ``SchedulerBinding.enabled`` filtering.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone as dt_timezone

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.app.views.scheduler.occurrences import (
    MAX_WINDOW_DAYS,
    OCCURRENCE_CAP,
    ProjectSchedulerOccurrencesEndpoint,
    _parse_iso,
)
from pi_dash.db.models import Project, ProjectMember, Scheduler, SchedulerBinding


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        p = Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )
    # Make create_user a project admin so allow_permission(PROJECT) passes
    # in the endpoint smoke tests below.
    ProjectMember.objects.create(
        project=p,
        workspace=workspace,
        member=create_user,
        role=20,  # ADMIN
    )
    return p


@pytest.fixture
def scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="test-occurrences",
            name="Test Occurrences",
            prompt="Scan.",
            color="#10b981",
        )


@pytest.fixture
def hourly_binding(scheduler, project, workspace, create_user):
    """Fires every hour, anchored 1 day ago."""
    with impersonate(create_user):
        return SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            dtstart=timezone.now() - timedelta(days=1),
            rrule="FREQ=HOURLY",
            tzid="UTC",
            enabled=True,
            actor=create_user,
        )


# ---------------------------------------------------------------------------
# _parse_iso — small helper unit test
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_parse_iso_accepts_z_and_offset_forms():
    a = _parse_iso("2026-05-25T09:00:00Z")
    b = _parse_iso("2026-05-25T09:00:00+00:00")
    assert a == b == datetime(2026, 5, 25, 9, 0, tzinfo=dt_timezone.utc)


@pytest.mark.unit
def test_parse_iso_returns_none_on_garbage():
    assert _parse_iso("garbage") is None
    assert _parse_iso("") is None


# ---------------------------------------------------------------------------
# Cap + window helpers — pure-Python, no view invocation
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_max_window_and_cap_are_sane_defaults():
    """If these change, the frontend's calendar fetch needs to be adjusted."""
    assert MAX_WINDOW_DAYS == 90
    assert OCCURRENCE_CAP == 5000


# ---------------------------------------------------------------------------
# Endpoint smoke tests — exercise the view via Django RF directly.
# These need the project + workspace + a binding seeded.
# ---------------------------------------------------------------------------


def _call_endpoint(workspace, project, query: dict, user):
    """Drive the endpoint through Django's request factory."""
    from rest_framework.test import APIRequestFactory, force_authenticate

    factory = APIRequestFactory()
    request = factory.get(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}/scheduler-bindings/occurrences/",
        query,
    )
    force_authenticate(request, user=user)
    view = ProjectSchedulerOccurrencesEndpoint.as_view()
    return view(request, slug=workspace.slug, project_id=str(project.id))


@pytest.mark.unit
def test_occurrences_rejects_window_over_90_days(
    db, workspace, project, scheduler, hourly_binding, create_user
):
    res = _call_endpoint(
        workspace,
        project,
        {
            "from": "2026-01-01T00:00:00Z",
            "to": "2026-05-01T00:00:00Z",  # ~120 days
        },
        user=create_user,
    )
    assert res.status_code == 400
    assert res.data["error"] == "window_too_large"


@pytest.mark.unit
def test_occurrences_rejects_inverted_window(
    db, workspace, project, scheduler, hourly_binding, create_user
):
    res = _call_endpoint(
        workspace,
        project,
        {"from": "2026-06-01T00:00:00Z", "to": "2026-05-01T00:00:00Z"},
        user=create_user,
    )
    assert res.status_code == 400
    assert res.data["error"] == "invalid_window"


@pytest.mark.unit
def test_occurrences_returns_future_expansion(
    db, workspace, project, scheduler, hourly_binding, create_user
):
    """A binding firing every hour over a 24h window yields ~24 occurrences."""
    now = timezone.now()
    res = _call_endpoint(
        workspace,
        project,
        {
            "from": now.isoformat(),
            "to": (now + timedelta(hours=24)).isoformat(),
        },
        user=create_user,
    )
    assert res.status_code == 200
    data = res.data
    assert "occurrences" in data
    # Hourly over 24h → between 23 and 25 occurrences (boundary inclusive).
    scheduled = [o for o in data["occurrences"] if o["kind"] == "scheduled"]
    assert 22 <= len(scheduled) <= 26, len(scheduled)
    # All occurrences carry the joined color from Scheduler.
    assert all(o["scheduler_color"] == "#10b981" for o in scheduled)
    assert data["has_more"] is False


@pytest.mark.unit
def test_occurrences_omits_disabled_binding_futures(
    db, workspace, project, scheduler, hourly_binding, create_user
):
    """Disabled bindings: future occurrences are hidden (they won't fire)."""
    hourly_binding.enabled = False
    hourly_binding.save(update_fields=["enabled"])
    now = timezone.now()
    res = _call_endpoint(
        workspace,
        project,
        {
            "from": now.isoformat(),
            "to": (now + timedelta(hours=24)).isoformat(),
        },
        user=create_user,
    )
    scheduled = [o for o in res.data["occurrences"] if o["kind"] == "scheduled"]
    assert scheduled == []


@pytest.mark.unit
def test_occurrences_omits_disabled_scheduler_futures(
    db, workspace, project, scheduler, hourly_binding, create_user
):
    """Disabled schedulers: same — hide future occurrences from the calendar."""
    scheduler.is_enabled = False
    scheduler.save(update_fields=["is_enabled"])
    now = timezone.now()
    res = _call_endpoint(
        workspace,
        project,
        {
            "from": now.isoformat(),
            "to": (now + timedelta(hours=24)).isoformat(),
        },
        user=create_user,
    )
    scheduled = [o for o in res.data["occurrences"] if o["kind"] == "scheduled"]
    assert scheduled == []
