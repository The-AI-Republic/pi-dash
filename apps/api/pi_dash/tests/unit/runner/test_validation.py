# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``validate_run_creation`` (design §6.5)."""

from __future__ import annotations

import pytest

from pi_dash.db.models import Project, User, Workspace, WorkspaceMember
from pi_dash.db.models.issue import Issue
from pi_dash.runner.models import Pod
from pi_dash.runner.services.validation import (
    RunCreationError,
    validate_run_creation,
)


@pytest.fixture
def other_user(db):
    from uuid import uuid4

    unique = uuid4().hex[:8]
    user = User.objects.create(
        email=f"other-{unique}@example.com",
        username=f"other_{unique}",
        first_name="O",
        last_name="Ther",
    )
    user.set_password("pw")
    user.save()
    return user


@pytest.fixture
def second_workspace(create_user):
    ws = Workspace.objects.create(
        name="Second WS", owner=create_user, slug="second-ws"
    )
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    return ws


@pytest.fixture
def project(workspace, create_user):
    return Project.objects.create(
        name="Demo", workspace=workspace, identifier="DEMO"
    )


@pytest.fixture
def issue_in_workspace(db, project, workspace, create_user):
    # Issue.save() requires a State to be present via its save() override.
    # For validation tests we only care about workspace/assigned_pod; bypass
    # state resolution by using .objects.create with project set — the save
    # override falls back silently if State model has no rows.
    return Issue.objects.create(
        project=project,
        workspace=workspace,
        name="Demo issue",
        created_by=create_user,
    )


@pytest.mark.unit
def test_requires_workspace(db, create_user):
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(create_user, workspace_id=None)
    assert exc.value.status == 400
    assert exc.value.code == "workspace_required"


@pytest.mark.unit
def test_non_member_rejected_403(db, other_user, workspace):
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(other_user, workspace_id=workspace.id)
    assert exc.value.status == 403
    assert exc.value.code == "not_workspace_member"


@pytest.mark.unit
def test_resolves_workspace_default_pod_when_no_pod_given(
    db, create_user, workspace
):
    ctx = validate_run_creation(create_user, workspace_id=workspace.id)
    assert ctx.pod.is_default is True
    assert ctx.pod.workspace_id == workspace.id
    assert ctx.created_by == create_user


@pytest.mark.unit
def test_explicit_pod_must_belong_to_workspace(
    db, create_user, workspace, second_workspace
):
    other_pod = Pod.default_for_workspace(second_workspace)
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user, workspace_id=workspace.id, pod_id=other_pod.id
        )
    assert exc.value.status == 400
    assert exc.value.code == "pod_workspace_mismatch"


@pytest.mark.unit
def test_soft_deleted_pod_rejected(db, create_user, workspace):
    from django.utils import timezone

    pod = Pod.default_for_workspace(workspace)
    pod.deleted_at = timezone.now()
    pod.is_default = False
    pod.save(update_fields=["deleted_at", "is_default"])
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user, workspace_id=workspace.id, pod_id=pod.id
        )
    assert exc.value.status == 400
    assert exc.value.code == "pod_missing"


@pytest.mark.unit
def test_work_item_must_belong_to_workspace(
    db, create_user, workspace, second_workspace, issue_in_workspace
):
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user,
            workspace_id=second_workspace.id,
            work_item_id=issue_in_workspace.id,
        )
    assert exc.value.status == 400
    assert exc.value.code == "work_item_workspace_mismatch"


@pytest.mark.unit
def test_work_item_picks_assigned_pod_over_default(
    db, create_user, workspace, issue_in_workspace
):
    # Create a second pod, pin the issue to it, and verify resolution prefers it.
    second_pod = Pod.objects.create(
        workspace=workspace, name="special-pod", created_by=create_user
    )
    issue_in_workspace.assigned_pod = second_pod
    issue_in_workspace.save(update_fields=["assigned_pod"])

    ctx = validate_run_creation(
        create_user,
        workspace_id=workspace.id,
        work_item_id=issue_in_workspace.id,
    )
    assert ctx.pod.pk == second_pod.pk


@pytest.mark.unit
def test_no_pod_available_returns_409(db, create_user, workspace):
    """When every pod in the workspace is soft-deleted, fall back fails."""
    from django.utils import timezone

    for pod in Pod.all_objects.filter(workspace=workspace):
        pod.deleted_at = timezone.now()
        pod.is_default = False
        pod.save(update_fields=["deleted_at", "is_default"])
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(create_user, workspace_id=workspace.id)
    assert exc.value.status == 409
    assert exc.value.code == "no_pod_available"
