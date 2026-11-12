# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression coverage for the legacy ``updated_at`` list filter.

``filter_updated_at`` used to predicate ``created_at__date``, so an issue
updated on a different day than it was created was mis-filtered. Both list
surfaces that share the helper (work items, intake) are exercised here.
"""

from datetime import datetime, timezone

import pytest
from rest_framework import status

from pi_dash.db.models import Intake, IntakeIssue, Issue, ProjectMember
from pi_dash.utils.issue_filters import filter_updated_at

DAY_A = "2026-01-05"
DAY_B = "2026-01-12"


def _range(day):
    return f"{day};after,{day};before"


@pytest.mark.unit
@pytest.mark.parametrize("prefix", ["", "issue__"])
def test_filter_updated_at_get_predicates_updated_column(prefix):
    issue_filter = filter_updated_at({"updated_at": _range(DAY_B)}, {}, "GET", prefix)
    assert issue_filter == {
        f"{prefix}updated_at__date__gte": DAY_B,
        f"{prefix}updated_at__date__lte": DAY_B,
    }


@pytest.mark.unit
@pytest.mark.parametrize("prefix", ["", "issue__"])
def test_filter_updated_at_post_predicates_updated_column(prefix):
    issue_filter = filter_updated_at({"updated_at": [f"{DAY_B};after", f"{DAY_B};before"]}, {}, "POST", prefix)
    assert issue_filter == {
        f"{prefix}updated_at__date__gte": DAY_B,
        f"{prefix}updated_at__date__lte": DAY_B,
    }


@pytest.fixture
def stale_issue(db, workspace, project, create_user):
    """An intake issue created on DAY_A and last updated on DAY_B."""
    ProjectMember.objects.get_or_create(project=project, workspace=workspace, member=create_user, defaults={"role": 20})
    intake = Intake.objects.create(name="Intake", project=project, workspace=workspace, is_default=True)
    issue = Issue.objects.create(name="Stale issue", project=project, workspace=workspace)
    IntakeIssue.objects.create(intake=intake, issue=issue, project=project, workspace=workspace)
    # ``update()`` bypasses ``auto_now``/``auto_now_add`` so the two
    # timestamps land on different days.
    Issue.objects.filter(pk=issue.pk).update(
        created_at=datetime(2026, 1, 5, 12, tzinfo=timezone.utc),
        updated_at=datetime(2026, 1, 12, 12, tzinfo=timezone.utc),
    )
    return issue


def _work_item_ids(client, workspace, project, day):
    url = f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/"
    response = client.get(url, {"updated_at": _range(day)})
    assert response.status_code == status.HTTP_200_OK, response.data
    return {str(row["id"]) for row in response.data["results"]}


def _intake_issue_ids(client, workspace, project, day):
    url = f"/api/workspaces/{workspace.slug}/projects/{project.id}/intake-issues/"
    response = client.get(url, {"updated_at": _range(day)})
    assert response.status_code == status.HTTP_200_OK, response.data
    return {str(row["issue"]["id"]) for row in response.data["results"]}


@pytest.mark.contract
@pytest.mark.django_db
def test_work_item_list_updated_at_filter_uses_updated_day(session_client, workspace, project, stale_issue):
    assert str(stale_issue.id) in _work_item_ids(session_client, workspace, project, DAY_B)
    assert str(stale_issue.id) not in _work_item_ids(session_client, workspace, project, DAY_A)


@pytest.mark.contract
@pytest.mark.django_db
def test_intake_list_updated_at_filter_uses_updated_day(session_client, workspace, project, stale_issue):
    assert str(stale_issue.id) in _intake_issue_ids(session_client, workspace, project, DAY_B)
    assert str(stale_issue.id) not in _intake_issue_ids(session_client, workspace, project, DAY_A)
