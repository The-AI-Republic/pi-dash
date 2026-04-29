# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the project-scoped ``Issue.assigned_pod`` flow.

Covers:

- ``Issue.save()``'s auto-resolution flips from workspace-default to
  project-default on create.
- The serializer's ``validate_assigned_pod`` rejects a pod from a
  different project (the cross-project escape hatch the original
  workspace-equality guard left open).

See ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§5.5 and §8.
"""

from __future__ import annotations

import pytest
from django.urls import reverse

from pi_dash.app.serializers.issue import IssueCreateSerializer, IssueSerializer
from pi_dash.db.models.issue import Issue
from pi_dash.db.models.project import Project
from pi_dash.db.models.state import State
from pi_dash.runner.models import Pod


@pytest.fixture
def state(workspace, project, create_user):
    return State.objects.create(
        name="Backlog",
        group="backlog",
        workspace=workspace,
        project=project,
        sequence=1.0,
        default=True,
    )


@pytest.fixture
def second_project(workspace, create_user):
    return Project.objects.create(
        name="Second", identifier="OTHER", workspace=workspace, created_by=create_user
    )


@pytest.mark.unit
def test_issue_save_auto_assigns_project_default_pod(
    db, project, state, create_user
):
    issue = Issue.objects.create(
        name="An issue",
        project=project,
        workspace=project.workspace,
        state=state,
        created_by=create_user,
    )
    issue.refresh_from_db()
    expected = Pod.default_for_project(project)
    assert expected is not None
    assert issue.assigned_pod_id == expected.id


@pytest.mark.unit
def test_issue_serializer_rejects_cross_project_pod(
    db, project, second_project, state, create_user
):
    """Project P's issue cannot be assigned Project Q's pod, even if both
    pods are in the same workspace. The original workspace-equality guard
    let this through; the new project-equality guard must block it.
    """
    issue = Issue.objects.create(
        name="An issue",
        project=project,
        workspace=project.workspace,
        state=state,
        created_by=create_user,
    )
    other_pod = Pod.default_for_project(second_project)

    serializer = IssueSerializer(
        instance=issue,
        data={"assigned_pod": str(other_pod.id)},
        partial=True,
        context={"project_id": str(project.id)},
    )
    assert not serializer.is_valid()
    err = serializer.errors.get("assigned_pod") or serializer.errors
    assert any("different project" in str(e) for e in err)


@pytest.mark.unit
def test_issue_serializer_accepts_same_project_pod(
    db, project, state, create_user
):
    """Same-project pod IS acceptable — sanity for the happy path."""
    issue = Issue.objects.create(
        name="An issue",
        project=project,
        workspace=project.workspace,
        state=state,
        created_by=create_user,
    )
    same_pod = Pod.default_for_project(project)

    serializer = IssueSerializer(
        instance=issue,
        data={"assigned_pod": str(same_pod.id)},
        partial=True,
        context={"project_id": str(project.id)},
    )
    assert serializer.is_valid(), serializer.errors
