# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Replaces the legacy ``test_workspace_signal.py`` from the
workspace-default-pod model. Pods are now project-scoped; the
``post_save(Workspace)`` handler is gone. The tests here verify the
*absence* of that handler so a future regression doesn't sneak it back
in.

Project-scoped pod auto-creation is covered in ``test_pod.py``.
"""

from __future__ import annotations

import pytest

from pi_dash.db.models import Workspace
from pi_dash.runner.models import Pod


@pytest.mark.unit
def test_workspace_creation_does_not_auto_create_a_pod(db, create_user):
    ws = Workspace.objects.create(
        name="Ws No Auto Pod",
        owner=create_user,
        slug="ws-no-auto-pod",
    )
    # Pre-refactor: a workspace-default pod was auto-created here. Now: nothing.
    # Pods only exist as a side-effect of Project creation, which this test
    # deliberately skips.
    assert Pod.objects.filter(workspace=ws).count() == 0


@pytest.mark.unit
def test_workspace_default_pod_lookup_was_removed(db, create_user):
    """The legacy ``Pod.default_for_workspace`` API is gone.

    Catching this at attribute level guarantees no stragglers are still
    calling the old helper after the refactor.
    """
    assert not hasattr(Pod, "default_for_workspace")
    assert not hasattr(Pod, "default_for_workspace_id")
    assert hasattr(Pod, "default_for_project")
    assert hasattr(Pod, "default_for_project_id")
