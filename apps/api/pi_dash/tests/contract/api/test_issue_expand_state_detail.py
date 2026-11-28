# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Paginated v1 lists that expand their work item (``?expand=issue``) render
it with ``IssueSerializer``, whose ``state_detail`` (PDASHOSS01-404) reads
``issue.state``. Each row carries its own issue instance, so the list
querysets must load ``issue__state`` with the page: no per-row state query.
"""

import re

import pytest
from django.db import connection
from django.test.utils import CaptureQueriesContext
from rest_framework import status

from pi_dash.db.models import (
    Intake,
    IntakeIssue,
    Issue,
    IssueActivity,
    IssueComment,
    IssueLink,
    Project,
    ProjectMember,
    State,
)

ROWS = 3

# A query that reads the states table on its own (not as a JOIN of the page query).
_STATE_LOOKUP = re.compile(r'^\s*SELECT .* FROM "states"', re.IGNORECASE | re.DOTALL)


@pytest.fixture
def proj(db, workspace, create_user):
    project = Project.objects.create(
        name="Expand Project", identifier="EXP", workspace=workspace, created_by=create_user
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def issue(proj, create_user):
    state = State.objects.create(
        name="Shipped", project=proj, workspace=proj.workspace, group="completed", created_by=create_user
    )
    return Issue.objects.create(
        name="Expanded", project=proj, workspace=proj.workspace, state=state, created_by=create_user
    )


def _base(workspace, proj, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{proj.id}/issues/{issue.id}"


def _seed_comments(issue, user):
    for i in range(ROWS):
        IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=user,
            comment_html=f"<p>c{i}</p>",
            comment_stripped=f"c{i}",
        )


def _seed_links(issue, user):
    for i in range(ROWS):
        IssueLink.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            url=f"https://example.com/{i}",
            title=f"l{i}",
            created_by=user,
        )


def _seed_activities(issue, user):
    for i in range(ROWS):
        IssueActivity.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=user,
            verb="updated",
            field="name",
            old_value=f"n{i}",
            new_value=f"n{i + 1}",
        )


@pytest.mark.contract
@pytest.mark.parametrize(
    "path,seed",
    [("comments", _seed_comments), ("links", _seed_links), ("activities", _seed_activities)],
)
def test_expand_issue_loads_state_with_the_page(api_key_client, workspace, proj, issue, create_user, path, seed):
    seed(issue, create_user)
    with CaptureQueriesContext(connection) as ctx:
        response = api_key_client.get(f"{_base(workspace, proj, issue)}/{path}/", {"expand": "issue"})
    assert response.status_code == status.HTTP_200_OK
    rows = response.data["results"]
    assert len(rows) >= ROWS
    for row in rows:
        assert row["issue"]["state_detail"] == {"name": "Shipped", "group": "completed"}
    state_lookups = [q["sql"] for q in ctx.captured_queries if _STATE_LOOKUP.match(q["sql"])]
    assert state_lookups == []


@pytest.mark.contract
def test_intake_list_expand_issue_loads_state_with_the_page(api_key_client, workspace, proj, issue, create_user):
    Project.objects.filter(pk=proj.pk).update(intake_view=True)
    intake = Intake.objects.create(name="Intake", project=proj, workspace=proj.workspace, created_by=create_user)
    for i in range(ROWS):
        item = Issue.objects.create(
            name=f"Intake {i}", project=proj, workspace=proj.workspace, state=issue.state, created_by=create_user
        )
        IntakeIssue.objects.create(intake=intake, issue=item, project=proj, workspace=proj.workspace)
    with CaptureQueriesContext(connection) as ctx:
        response = api_key_client.get(
            f"/api/v1/workspaces/{workspace.slug}/projects/{proj.id}/intake-issues/", {"expand": "issue"}
        )
    assert response.status_code == status.HTTP_200_OK
    rows = response.data["results"]
    assert len(rows) == ROWS
    for row in rows:
        assert row["issue"]["state_detail"] == {"name": "Shipped", "group": "completed"}
    state_lookups = [q["sql"] for q in ctx.captured_queries if _STATE_LOOKUP.match(q["sql"])]
    assert state_lookups == []
