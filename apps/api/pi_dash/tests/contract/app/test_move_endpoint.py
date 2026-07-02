# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the session-authed ``IssueMoveEndpoint``.

This is the web-app counterpart to ``tests/contract/api/test_move_endpoint.py``
(the API-key surface the CLI uses). Both endpoints delegate to
``pi_dash.utils.issue_move.move_work_item_to_project``, so this suite guards the
web-app wiring: session auth, the ``/api/`` (non-``v1``) route, and the
``@allow_permission`` decorator.
"""

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    Project,
    ProjectMember,
    State,
)


@pytest.fixture
def source_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Move Source Project",
        identifier="MVSRC",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )
    return project


@pytest.fixture
def target_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Move Target Project",
        identifier="MVDST",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )
    # The move requires a non-triage default workflow state on the target,
    # otherwise it short-circuits with a 400.
    State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )
    return project


def _make_issue(project, user, *, state=None):
    return Issue.objects.create(
        name="move target",
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=project.workspace,
        state=state,
        created_by=user,
    )


def _url(slug, project_id, issue_id):
    # The web app is served under ``/api/`` (no ``v1``) with session auth.
    return f"/api/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/move/"


@pytest.mark.contract
class TestIssueMoveApp:
    @pytest.mark.django_db
    def test_move_returns_200_and_reassigns_project(
        self, session_client, workspace, source_project, target_project, create_user
    ):
        issue = _make_issue(source_project, create_user)
        url = _url(workspace.slug, source_project.id, issue.id)

        response = session_client.post(url, {"project": target_project.identifier}, format="json")

        assert response.status_code == http_status.HTTP_200_OK
        issue.refresh_from_db()
        assert str(issue.project_id) == str(target_project.id)

    @pytest.mark.django_db
    def test_move_requires_target_project(self, session_client, workspace, source_project, create_user):
        issue = _make_issue(source_project, create_user)
        url = _url(workspace.slug, source_project.id, issue.id)

        response = session_client.post(url, {}, format="json")

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_move_rejects_non_member_target(self, session_client, workspace, source_project, create_user):
        # A target the acting user is not a member of must be rejected.
        outsider_project = Project.objects.create(
            name="Move Outsider Project",
            identifier="MVOUT",
            workspace=workspace,
            created_by=create_user,
        )
        State.objects.create(
            name="Todo",
            project=outsider_project,
            workspace=workspace,
            group="unstarted",
            default=True,
            created_by=create_user,
        )
        issue = _make_issue(source_project, create_user)
        url = _url(workspace.slug, source_project.id, issue.id)

        response = session_client.post(url, {"project": outsider_project.identifier}, format="json")

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        issue.refresh_from_db()
        assert str(issue.project_id) == str(source_project.id)
