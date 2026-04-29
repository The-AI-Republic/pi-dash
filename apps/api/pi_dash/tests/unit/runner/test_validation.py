# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``validate_run_creation`` (project-scoped pod resolution).

Covers the rules in
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§8 and ``services/validation.py``.
"""

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
def second_project(workspace, create_user):
    return Project.objects.create(
        name="Demo2",
        workspace=workspace,
        identifier="DEMO2",
        created_by=create_user,
    )


@pytest.fixture
def issue_in_workspace(db, project, workspace, create_user):
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
def test_no_work_item_returns_409_no_pod_available(
    db, create_user, workspace
):
    """Without a ``work_item``, ``validate_run_creation`` has no project to
    anchor pod resolution to. Post-refactor it returns 409 instead of
    silently falling back to a workspace-default pod (which doesn't exist).
    """
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(create_user, workspace_id=workspace.id)
    assert exc.value.status == 409
    assert exc.value.code == "no_pod_available"


@pytest.mark.unit
def test_work_item_resolves_to_project_default_pod(
    db, create_user, workspace, project, issue_in_workspace
):
    ctx = validate_run_creation(
        create_user,
        workspace_id=workspace.id,
        work_item_id=issue_in_workspace.id,
    )
    expected = Pod.default_for_project(project)
    assert ctx.pod.pk == expected.pk
    assert ctx.created_by == create_user


@pytest.mark.unit
def test_explicit_pod_must_belong_to_workspace(
    db, create_user, workspace, second_workspace
):
    other_project = Project.objects.create(
        name="Other",
        workspace=second_workspace,
        identifier="OTHER",
        created_by=create_user,
    )
    other_pod = Pod.default_for_project(other_project)
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user, workspace_id=workspace.id, pod_id=other_pod.id
        )
    assert exc.value.status == 400
    assert exc.value.code == "pod_workspace_mismatch"


@pytest.mark.unit
def test_explicit_pod_must_belong_to_issue_project(
    db, create_user, workspace, project, second_project, issue_in_workspace
):
    """Cross-project pod with an explicit pod_id is rejected."""
    other_pod = Pod.default_for_project(second_project)
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user,
            workspace_id=workspace.id,
            work_item_id=issue_in_workspace.id,
            pod_id=other_pod.id,
        )
    assert exc.value.status == 400
    assert exc.value.code == "pod_project_mismatch"


@pytest.mark.unit
def test_soft_deleted_pod_rejected(db, create_user, project):
    from django.utils import timezone

    pod = Pod.default_for_project(project)
    pod.deleted_at = timezone.now()
    pod.is_default = False
    pod.save(update_fields=["deleted_at", "is_default"])
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user, workspace_id=project.workspace_id, pod_id=pod.id
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
    db, create_user, workspace, project, issue_in_workspace
):
    """An issue's pinned ``assigned_pod`` wins over the project default
    when the pod is in the same project.
    """
    second_pod = Pod.objects.create(
        workspace=workspace,
        project=project,
        name=f"{project.identifier}_special",
        created_by=create_user,
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
def test_no_pod_available_when_default_pod_soft_deleted(
    db, create_user, workspace, project, issue_in_workspace
):
    from django.utils import timezone

    issue_in_workspace.assigned_pod = None
    issue_in_workspace.save(update_fields=["assigned_pod"])
    for pod in Pod.all_objects.filter(project=project):
        pod.deleted_at = timezone.now()
        pod.is_default = False
        pod.save(update_fields=["deleted_at", "is_default"])
    with pytest.raises(RunCreationError) as exc:
        validate_run_creation(
            create_user,
            workspace_id=workspace.id,
            work_item_id=issue_in_workspace.id,
        )
    assert exc.value.status == 409
    assert exc.value.code == "no_pod_available"
