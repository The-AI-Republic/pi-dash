# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the issue ↔ pod integration (Phase 4 of the design)."""

from __future__ import annotations

from uuid import uuid4

import pytest

from pi_dash.db.models import (
    Project,
    State,
    User,
    Workspace,
    WorkspaceMember,
)
from pi_dash.db.models.issue import Issue
from pi_dash.runner.models import Pod


@pytest.fixture
def project(workspace):
    return Project.objects.create(
        name="ProjP4", workspace=workspace, identifier="P4"
    )


@pytest.fixture
def state(project):
    return State.objects.create(
        name="Backlog",
        project=project,
        workspace=project.workspace,
        group="backlog",
        default=True,
    )


@pytest.fixture
def second_workspace(create_user):
    ws = Workspace.objects.create(
        name="OtherWS-p4", owner=create_user, slug="other-p4"
    )
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    return ws


@pytest.mark.unit
def test_new_issue_auto_fills_assigned_pod_from_project_default(
    db, create_user, project, state
):
    issue = Issue.objects.create(
        name="Demo",
        project=project,
        workspace=project.workspace,
        created_by=create_user,
    )
    assert issue.assigned_pod_id == Pod.default_for_project(project).id


@pytest.mark.unit
def test_explicit_assigned_pod_preserved_on_create(
    db, create_user, project, state
):
    """If the caller passes an explicit pod, the auto-resolve doesn't overwrite it."""
    custom = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_custom",
        created_by=create_user,
    )
    issue = Issue.objects.create(
        name="Pinned",
        project=project,
        workspace=project.workspace,
        created_by=create_user,
        assigned_pod=custom,
    )
    assert issue.assigned_pod_id == custom.id


@pytest.mark.unit
def test_create_serializer_rejects_pod_in_other_project(
    db, create_user, project, state
):
    """Cross-project pod is rejected. Same-workspace, different-project."""
    from pi_dash.app.serializers.issue import IssueCreateSerializer
    from pi_dash.db.models.project import Project

    other_project = Project.objects.create(
        name="Other",
        identifier="OTHERPP",
        workspace=project.workspace,
        created_by=create_user,
    )
    other_pod = Pod.default_for_project(other_project)
    serializer = IssueCreateSerializer(
        data={
            "name": "Bad",
            "assigned_pod": str(other_pod.id),
        },
        context={
            "project_id": project.id,
            "workspace_id": project.workspace_id,
        },
    )
    assert serializer.is_valid() is False
    assert "assigned_pod" in serializer.errors


@pytest.mark.unit
def test_create_serializer_rejects_soft_deleted_pod(
    db, create_user, project, state
):
    from django.utils import timezone

    from pi_dash.app.serializers.issue import IssueCreateSerializer

    extra = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name=f"{project.identifier}_extra",
        created_by=create_user,
    )
    extra.deleted_at = timezone.now()
    extra.save(update_fields=["deleted_at"])

    serializer = IssueCreateSerializer(
        data={"name": "Bad2", "assigned_pod": str(extra.id)},
        context={
            "project_id": project.id,
            "workspace_id": project.workspace_id,
        },
    )
    assert serializer.is_valid() is False
    assert "assigned_pod" in serializer.errors


@pytest.mark.unit
def test_assigned_pod_detail_method_returns_pod_data(
    db, create_user, project, state
):
    """get_assigned_pod_detail returns the nested pod info or None.

    Doesn't go through the full serializer because IssueSerializer has many
    nested fields requiring annotated querysets. The method is the bit we
    added in Phase 4; we exercise it directly.
    """
    from pi_dash.space.serializer.issue import IssueSerializer

    issue = Issue.objects.create(
        name="Show",
        project=project,
        workspace=project.workspace,
        created_by=create_user,
    )
    serializer = IssueSerializer()
    data = serializer.get_assigned_pod_detail(issue)
    assert data is not None
    assert data["is_default"] is True
    # Pod name now follows project-scoped convention `{identifier}_pod_1`
    # (was: workspace-name-derived). See Phase A signal change.
    assert data["name"].endswith("_pod_1")


@pytest.mark.unit
def test_assigned_pod_detail_returns_none_when_unassigned(
    db, create_user, project, state
):
    from pi_dash.space.serializer.issue import IssueSerializer

    issue = Issue.objects.create(
        name="Unpinned",
        project=project,
        workspace=project.workspace,
        created_by=create_user,
    )
    issue.assigned_pod = None
    issue.save(update_fields=["assigned_pod"])
    serializer = IssueSerializer()
    assert serializer.get_assigned_pod_detail(issue) is None
