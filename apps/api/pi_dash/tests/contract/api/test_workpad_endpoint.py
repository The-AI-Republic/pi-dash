# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for ``IssueWorkpadAPIEndpoint`` (the agent workpad).

Regression for the workpad-write 500: ``PATCH .../workpad/`` locks the row
with ``select_for_update`` through the ``Issue.issue_objects`` manager, which
excludes triage states via ``.exclude(state__group=...)``. Because ``state``
is a nullable FK that produces a LEFT OUTER JOIN, a bare ``FOR UPDATE`` asks
Postgres to lock the nullable side of an outer join — which it refuses (the fix scopes the lock to the base
``issues`` row via ``of=("self",)``), so
every ``pidash workpad update`` returned HTTP 500. Reads (GET) and other issue
writes use a plain ``.get()`` and were unaffected, which is why the failure was
isolated to the workpad write path.

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
def wp_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Workpad Test Project",
        identifier="WP",
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


def _make_issue(project, user, *, state=None):
    return Issue.objects.create(
        name="workpad target",
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
        f"/work-items/{issue_id}/workpad/"
    )


@pytest.mark.contract
class TestIssueWorkpadPatch:
    @pytest.mark.django_db
    def test_patch_persists_body_and_returns_200(
        self, api_key_client, workspace, wp_project, create_user
    ):
        """The core regression: a valid ``{"body": ...}`` PATCH must lock,
        write, and return 200 — not 500 from ``FOR UPDATE`` on the manager's
        outer join.
        """
        issue = _make_issue(wp_project, create_user)
        url = _url(workspace.slug, wp_project.id, issue.id)

        response = api_key_client.patch(
            url, {"body": "## Phase\n- implementing\n"}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert "updated_at" in response.data
        issue.refresh_from_db()
        assert "implementing" in issue.workpad

    @pytest.mark.django_db
    def test_patch_locks_through_outer_join_with_state(
        self, api_key_client, workspace, wp_project, create_user
    ):
        """The outer join to ``states`` is present whether or not ``state`` is
        NULL, so an issue *with* a state must also lock cleanly under the fix.
        """
        state = State.objects.create(
            name="Todo",
            project=wp_project,
            workspace=workspace,
            group="unstarted",
            default=True,
            created_by=create_user,
        )
        issue = _make_issue(wp_project, create_user, state=state)
        url = _url(workspace.slug, wp_project.id, issue.id)

        response = api_key_client.patch(url, {"body": "locked ok"}, format="json")

        assert response.status_code == http_status.HTTP_200_OK
        issue.refresh_from_db()
        assert issue.workpad == "locked ok"

    @pytest.mark.django_db
    def test_patch_empty_body_clears_workpad(
        self, api_key_client, workspace, wp_project, create_user
    ):
        issue = _make_issue(wp_project, create_user)
        issue.workpad = "existing"
        issue.save()
        url = _url(workspace.slug, wp_project.id, issue.id)

        response = api_key_client.patch(url, {"body": ""}, format="json")

        assert response.status_code == http_status.HTTP_200_OK
        issue.refresh_from_db()
        assert issue.workpad == ""

    @pytest.mark.django_db
    def test_patch_missing_body_returns_400(
        self, api_key_client, workspace, wp_project, create_user
    ):
        """Wrong wire key (model name instead of ``body``) is rejected
        explicitly rather than silently no-op'ing under ``partial=True``.
        """
        issue = _make_issue(wp_project, create_user)
        url = _url(workspace.slug, wp_project.id, issue.id)

        response = api_key_client.patch(url, {"workpad": "wrong key"}, format="json")

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
