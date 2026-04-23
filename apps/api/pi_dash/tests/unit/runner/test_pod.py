# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Pod model / manager / constraint tests.

Covers the schema guarantees defined in
``.ai_design/issue_runner/design.md`` §4.1.
"""

from __future__ import annotations

import pytest
from django.db import IntegrityError, transaction
from django.utils import timezone

from pi_dash.db.models import Workspace
from pi_dash.runner.models import Pod


@pytest.fixture
def second_workspace(create_user):
    return Workspace.objects.create(
        name="Second Workspace", owner=create_user, slug="second-workspace"
    )


@pytest.mark.unit
def test_workspace_creation_auto_creates_default_pod(db, create_user):
    ws = Workspace.objects.create(
        name="Auto Pod Ws", owner=create_user, slug="auto-pod-ws"
    )
    pods = list(Pod.objects.filter(workspace=ws))
    assert len(pods) == 1
    assert pods[0].name == "Auto Pod Ws-pod"
    assert pods[0].is_default is True
    assert pods[0].created_by_id == create_user.id


@pytest.mark.unit
def test_workspace_creation_is_idempotent_for_pre_seeded_workspaces(
    db, create_user, workspace
):
    # workspace fixture already triggered the signal; a second pod row for the
    # same workspace+name+is_default=True should be forbidden by the
    # conditional unique constraint.
    existing = Pod.objects.filter(workspace=workspace, is_default=True).count()
    assert existing == 1


@pytest.mark.unit
def test_default_for_workspace_returns_active_default(db, workspace):
    pod = Pod.default_for_workspace(workspace)
    assert pod is not None
    assert pod.workspace_id == workspace.id
    assert pod.is_default is True


@pytest.mark.unit
def test_pod_manager_excludes_soft_deleted(db, workspace):
    pod = Pod.default_for_workspace(workspace)
    pod.deleted_at = timezone.now()
    pod.is_default = False  # clear default while soft-deleting
    pod.save(update_fields=["deleted_at", "is_default"])

    assert Pod.objects.filter(pk=pod.pk).count() == 0
    # all_objects still exposes the tombstone for audit.
    assert Pod.all_objects.filter(pk=pod.pk).count() == 1


@pytest.mark.unit
def test_duplicate_name_in_same_workspace_fails_when_both_active(
    db, create_user, workspace
):
    Pod.objects.create(
        workspace=workspace, name="dup", created_by=create_user
    )
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            Pod.objects.create(
                workspace=workspace, name="dup", created_by=create_user
            )


@pytest.mark.unit
def test_duplicate_name_allowed_when_prior_is_soft_deleted(
    db, create_user, workspace
):
    first = Pod.objects.create(
        workspace=workspace, name="reusable", created_by=create_user
    )
    first.deleted_at = timezone.now()
    first.save(update_fields=["deleted_at"])
    # Should succeed — conditional unique excludes deleted rows.
    second = Pod.objects.create(
        workspace=workspace, name="reusable", created_by=create_user
    )
    assert second.pk != first.pk


@pytest.mark.unit
def test_only_one_active_default_pod_per_workspace(
    db, create_user, workspace
):
    # The auto-created default already exists; adding another is_default=True
    # active pod should violate the conditional unique.
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            Pod.objects.create(
                workspace=workspace,
                name="second-default",
                is_default=True,
                created_by=create_user,
            )


@pytest.mark.unit
def test_default_pod_isolation_between_workspaces(
    db, workspace, second_workspace
):
    p1 = Pod.default_for_workspace(workspace)
    p2 = Pod.default_for_workspace(second_workspace)
    assert p1 is not None and p2 is not None
    assert p1.pk != p2.pk
    assert p1.name != p2.name  # different workspace names
