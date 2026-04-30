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
def test_issue_create_serializer_rejects_cross_project_pod(
    db, project, second_project, state, create_user
):
    """Project P's issue cannot be created with Project Q's pod, even if
    both pods are in the same workspace. The original workspace-equality
    guard let this through; the new project-equality guard must block it.
    """
    other_pod = Pod.default_for_project(second_project)

    serializer = IssueCreateSerializer(
        data={
            "name": "An issue",
            "project": str(project.id),
            "state": str(state.id),
            "assigned_pod": str(other_pod.id),
        },
        context={
            "project_id": str(project.id),
            "workspace_id": str(project.workspace_id),
        },
    )
    valid = serializer.is_valid()
    assert not valid, serializer.errors
    err = serializer.errors.get("assigned_pod") or serializer.errors
    assert any("different project" in str(e) for e in err)


@pytest.mark.unit
def test_issue_create_serializer_accepts_same_project_pod(
    db, project, state, create_user
):
    """Same-project pod IS acceptable — full happy path.

    A complete payload is constructed so ``is_valid()`` should be
    strictly True. A regression that flips the project guard to fire on
    same-project pods is caught here — the previous "soft" form
    (assert no `different project` error) would have silently passed
    if any unrelated field also failed.
    """
    same_pod = Pod.default_for_project(project)

    # ``crum.impersonate`` populates the user that ``BaseSerializer``
    # injects into ``created_by`` / ``updated_by`` during validation;
    # without it ``is_valid()`` fails on those fields and the test
    # can't tell apart "guard didn't fire" from "other field failed."
    from crum import impersonate

    with impersonate(create_user):
        serializer = IssueCreateSerializer(
            data={
                "name": "An issue",
                "project": str(project.id),
                "state": str(state.id),
                "assigned_pod": str(same_pod.id),
            },
            context={
                "project_id": str(project.id),
                "workspace_id": str(project.workspace_id),
            },
        )
        assert serializer.is_valid(), serializer.errors
