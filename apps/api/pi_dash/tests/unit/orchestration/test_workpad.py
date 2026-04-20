# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import Issue, IssueComment, Project, State
from pi_dash.orchestration.workpad import (
    WORKPAD_MARKER,
    get_agent_system_user,
    get_or_create_workpad,
    update_workpad_body,
)


@pytest.fixture
def project(db, workspace, create_user):
    return Project.objects.create(
        name="P", identifier="P", workspace=workspace, created_by=create_user
    )


@pytest.fixture
def state(project):
    # Non-trigger state so Issue creation doesn't fire orchestration.
    return State.objects.create(name="Todo", project=project, group="unstarted")


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="Task", workspace=workspace, project=project, state=state, created_by=create_user
    )


@pytest.mark.unit
def test_creates_workpad_on_first_call(issue):
    workpad = get_or_create_workpad(issue)
    assert workpad.pk is not None
    assert workpad.comment_stripped.startswith(WORKPAD_MARKER)
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_reuses_existing_workpad(issue):
    first = get_or_create_workpad(issue)
    second = get_or_create_workpad(issue)
    assert first.pk == second.pk
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_update_replaces_body(issue):
    get_or_create_workpad(issue)
    updated = update_workpad_body(issue, f"{WORKPAD_MARKER}\n\n### Phase\n- implementing")
    updated.refresh_from_db()
    assert "implementing" in updated.comment_stripped
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_agent_user_is_bot(db):
    user = get_agent_system_user()
    assert user.is_bot is True
    # idempotent
    again = get_agent_system_user()
    assert again.pk == user.pk
