# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for ``IssueMoveAPIEndpoint`` (move a work item across projects).

Regression for the same ``FOR UPDATE``-on-outer-join 500 fixed for the workpad
write path: ``POST .../move/`` locks the source row via
``Issue.issue_objects.select_for_update()``. That manager excludes triage states
through ``.exclude(state__group=...)`` and ``state`` is a nullable FK, so the
queryset carries a LEFT OUTER JOIN to ``states``. A bare ``FOR UPDATE`` asks
Postgres to lock the nullable side of that join — which it refuses — so every
move returned HTTP 500. The fix scopes the lock to the base ``issues`` row via
``of=("self",)``.

These need a real Postgres row lock, so they run under ``pytest.mark.contract``
with the standard ``django_db_setup`` surface (same as ``test_issue_search.py``).
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
    # The move endpoint requires a non-triage default workflow state on the
    # target, otherwise it short-circuits with a 400 before reaching the lock.
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
    return (
        f"/api/v1/workspaces/{slug}/projects/{project_id}"
        f"/work-items/{issue_id}/move/"
    )


@pytest.mark.contract
class TestIssueMovePost:
    @pytest.mark.django_db
    def test_move_locks_through_outer_join_and_returns_200(
        self, api_key_client, workspace, source_project, target_project, create_user
    ):
        """The core regression: moving a (null-state) issue must lock, move,
        and return 200 — not 500 from ``FOR UPDATE`` on the manager's outer
        join.
        """
        issue = _make_issue(source_project, create_user)
        url = _url(workspace.slug, source_project.id, issue.id)

        response = api_key_client.post(
            url, {"project": target_project.identifier}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        issue.refresh_from_db()
        assert str(issue.project_id) == str(target_project.id)
