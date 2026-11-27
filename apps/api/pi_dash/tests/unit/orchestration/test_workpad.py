# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import Issue, Project, State
from pi_dash.orchestration.workpad import (
    get_agent_system_user,
    get_workpad,
    set_workpad,
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
def test_workpad_empty_by_default(issue):
    assert get_workpad(issue) == ""


@pytest.mark.unit
def test_agent_system_user_is_pi_dash_bot(db):
    # §6.6 (decision 1): last name "Bot", explicit display name
    # "Pi Dash Bot", username unchanged.
    from pi_dash.orchestration import workpad

    user = get_agent_system_user()
    assert workpad.AGENT_USER_LAST_NAME == "Bot"
    assert user.username == "pi_dash_agent"
    assert user.first_name == "Pi Dash"
    assert user.last_name == "Bot"
    assert user.display_name == "Pi Dash Bot"
    assert user.is_bot


@pytest.mark.unit
def test_bot_prompt_label_is_pi_dash_bot(db):
    # §6.6 a.2: bot actors render as "AI agent: Pi Dash Bot".
    from types import SimpleNamespace

    from pi_dash.prompting.context import _comment_author_label

    bot = SimpleNamespace(display_name="Pi Dash Bot", is_bot=True)
    comment = SimpleNamespace(actor=bot, speaker_type="human", speaker_label="")
    assert _comment_author_label(comment) == "AI agent: Pi Dash Bot"


@pytest.mark.unit
def test_set_workpad_persists_body(issue):
    set_workpad(issue, "## Agent Workpad\n\n### Phase\n- implementing\n")
    issue.refresh_from_db()
    assert "implementing" in issue.workpad


@pytest.mark.unit
def test_set_workpad_overwrites(issue):
    set_workpad(issue, "first")
    set_workpad(issue, "second")
    issue.refresh_from_db()
    assert issue.workpad == "second"


@pytest.mark.unit
def test_set_workpad_clears_with_empty_string(issue):
    set_workpad(issue, "something")
    set_workpad(issue, "")
    issue.refresh_from_db()
    assert issue.workpad == ""


@pytest.mark.unit
def test_agent_user_is_bot(db):
    user = get_agent_system_user()
    assert user.is_bot is True
    # idempotent
    again = get_agent_system_user()
    assert again.pk == user.pk
