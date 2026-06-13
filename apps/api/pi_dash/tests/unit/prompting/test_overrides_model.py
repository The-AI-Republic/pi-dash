# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PromptSectionOverride model + the dual partial-unique constraints (§6.1)."""

from __future__ import annotations

import pytest
from django.db import IntegrityError, transaction

from pi_dash.prompting.models import PromptSectionOverride


@pytest.mark.unit
def test_workspace_level_is_workspace_level(db, workspace):
    row = PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body="x"
    )
    assert row.is_workspace_level


@pytest.mark.unit
def test_user_level_not_workspace_level(db, workspace, create_user):
    row = PromptSectionOverride.objects.create(
        workspace=workspace, user=create_user, section_key="implementation", body="x"
    )
    assert not row.is_workspace_level


@pytest.mark.unit
def test_duplicate_active_workspace_row_rejected(db, workspace):
    """The NULL-distinctness bug: a second ACTIVE workspace-level row for the
    same (workspace, section_key) must be blocked by the partial constraint."""
    PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body="a"
    )
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            PromptSectionOverride.objects.create(
                workspace=workspace, user=None, section_key="implementation", body="b"
            )


@pytest.mark.unit
def test_duplicate_active_user_row_rejected(db, workspace, create_user):
    PromptSectionOverride.objects.create(
        workspace=workspace, user=create_user, section_key="implementation", body="a"
    )
    with pytest.raises(IntegrityError):
        with transaction.atomic():
            PromptSectionOverride.objects.create(
                workspace=workspace,
                user=create_user,
                section_key="implementation",
                body="b",
            )


@pytest.mark.unit
def test_inactive_row_does_not_block_new_active(db, workspace):
    PromptSectionOverride.objects.create(
        workspace=workspace,
        user=None,
        section_key="implementation",
        body="old",
        is_active=False,
    )
    # A new active row is allowed alongside the archived one.
    PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body="new"
    )
    assert (
        PromptSectionOverride.objects.filter(
            workspace=workspace, section_key="implementation"
        ).count()
        == 2
    )


@pytest.mark.unit
def test_workspace_and_user_rows_coexist(db, workspace, create_user):
    # Same (workspace, section_key) but different scope → both allowed.
    PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body="ws"
    )
    PromptSectionOverride.objects.create(
        workspace=workspace, user=create_user, section_key="implementation", body="user"
    )
    assert (
        PromptSectionOverride.objects.filter(
            workspace=workspace, section_key="implementation", is_active=True
        ).count()
        == 2
    )
