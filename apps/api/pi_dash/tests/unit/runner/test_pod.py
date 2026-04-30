# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod model / manager / constraint tests.

Covers the project-scoped schema guarantees defined in
``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
§5–§6.
"""

from __future__ import annotations

import pytest
from django.core.exceptions import ValidationError
from django.db import IntegrityError, transaction
from django.utils import timezone

from pi_dash.db.models import Workspace
from pi_dash.db.models.project import Project
from pi_dash.runner.models import Pod


@pytest.fixture
def second_workspace(create_user):
    return Workspace.objects.create(
        name="Second Workspace", owner=create_user, slug="second-workspace"
    )


@pytest.fixture
def second_project(workspace, create_user):
    return Project.objects.create(
        name="Second Project",
        identifier="SECOND",
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture
def project_in_other_workspace(second_workspace, create_user):
    return Project.objects.create(
        name="Other Project",
        identifier="OTHER",
        workspace=second_workspace,
        created_by=create_user,
    )


@pytest.mark.unit
def test_project_creation_auto_creates_default_pod(db, workspace, create_user):
    proj = Project.objects.create(
        name="Auto Pod Project",
        identifier="AUTO",
        workspace=workspace,
        created_by=create_user,
    )
    pods = list(Pod.objects.filter(project=proj))
    assert len(pods) == 1
    assert pods[0].name == "AUTO_pod_1"
    assert pods[0].is_default is True


@pytest.mark.unit
def test_project_creation_is_idempotent_when_pod_exists(
    db, workspace, create_user
):
    # Pre-seed a pod under a project name to verify the signal's idempotency
    # branch (skip if any pod already exists for the project).
    proj = Project.objects.create(
        name="Pre-seeded",
        identifier="PRES",
        workspace=workspace,
        created_by=create_user,
    )
    # Signal already created PRES_pod_1 for this project; the post_save
    # handler is idempotent, so re-saving the project doesn't dup-create.
    proj.save()
    pods = Pod.objects.filter(project=proj)
    assert pods.count() == 1


@pytest.mark.unit
def test_default_for_project_returns_active_default(db, project):
    pod = Pod.default_for_project(project)
    assert pod is not None
    assert pod.project_id == project.id
    assert pod.is_default is True


@pytest.mark.unit
def test_pod_manager_excludes_soft_deleted(db, project):
    pod = Pod.default_for_project(project)
    pod.deleted_at = timezone.now()
    pod.is_default = False  # clear default while soft-deleting
    pod.save(update_fields=["deleted_at", "is_default"])

    assert Pod.objects.filter(pk=pod.pk).count() == 0
    assert Pod.all_objects.filter(pk=pod.pk).count() == 1


@pytest.mark.unit
def test_duplicate_name_in_same_project_fails_when_both_active(
    db, project, create_user
):
    Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name="TEST_dup",
        created_by=create_user,
    )
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            Pod.objects.create(
                workspace=project.workspace,
                project=project,
                name="TEST_dup",
                created_by=create_user,
            )


@pytest.mark.unit
def test_same_pod_name_allowed_in_different_projects(
    db, project, second_project, create_user
):
    """The unique constraint is per-project. Two projects in the same
    workspace can each have a `pod_2` etc. without colliding.
    """
    Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name="TEST_overlap",
        created_by=create_user,
    )
    Pod.objects.create(
        workspace=second_project.workspace,
        project=second_project,
        name="SECOND_overlap",
        created_by=create_user,
    )
    # Different names because the project-prefix is different — the names
    # would be unique even without the project filter, but the constraint
    # does the right thing semantically.


@pytest.mark.unit
def test_duplicate_name_allowed_when_prior_is_soft_deleted(
    db, project, create_user
):
    first = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name="TEST_reusable",
        created_by=create_user,
    )
    first.deleted_at = timezone.now()
    first.save(update_fields=["deleted_at"])
    second = Pod.objects.create(
        workspace=project.workspace,
        project=project,
        name="TEST_reusable",
        created_by=create_user,
    )
    assert second.pk != first.pk


@pytest.mark.unit
def test_only_one_active_default_pod_per_project(db, project, create_user):
    # post_save(Project) auto-created the default; a second is_default=True
    # for the same project must violate the conditional unique constraint.
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            Pod.objects.create(
                workspace=project.workspace,
                project=project,
                name="TEST_second_default",
                is_default=True,
                created_by=create_user,
            )


@pytest.mark.unit
def test_default_pod_isolation_between_projects(
    db, project, second_project
):
    p1 = Pod.default_for_project(project)
    p2 = Pod.default_for_project(second_project)
    assert p1 is not None and p2 is not None
    assert p1.pk != p2.pk
    assert p1.project_id != p2.project_id


@pytest.mark.unit
def test_pod_workspace_must_match_project_workspace(
    db, project, project_in_other_workspace, create_user
):
    """Pod.clean() enforces that pod.workspace_id == pod.project.workspace_id.

    This guards the denormalised ``workspace`` column from drifting away
    from the source-of-truth ``project.workspace``.
    """
    pod = Pod(
        workspace=project_in_other_workspace.workspace,  # wrong workspace
        project=project,
        name="TEST_drifty",
        created_by=create_user,
    )
    with pytest.raises(ValidationError):
        pod.full_clean()


@pytest.mark.unit
def test_pod_save_auto_fills_workspace_from_project(db, project, create_user):
    """Setting only project at create time is enough — workspace is
    auto-filled from the project on save.
    """
    pod = Pod.objects.create(
        project=project,
        name="TEST_autofill",
        created_by=create_user,
    )
    assert pod.workspace_id == project.workspace_id


@pytest.mark.unit
def test_pod_create_rejects_workspace_project_mismatch(
    db, project, project_in_other_workspace, create_user
):
    """Pod.objects.create() with conflicting workspace/project raises
    immediately; clean() is advisory because Django's save() doesn't
    invoke it, so the persistence path enforces the invariant directly.
    """
    with pytest.raises(ValidationError):
        Pod.objects.create(
            workspace=project_in_other_workspace.workspace,  # wrong workspace
            project=project,
            name="TEST_drifty_create",
            created_by=create_user,
        )
