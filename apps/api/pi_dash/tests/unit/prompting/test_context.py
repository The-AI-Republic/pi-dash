# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import Issue, Project, State
from pi_dash.prompting.context import build_context
from pi_dash.runner.models import AgentRun


@pytest.fixture
def project(db, workspace, create_user):
    return Project.objects.create(
        name="Test Project",
        identifier="TP",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/web.git",
        base_branch="trunk",
    )


@pytest.fixture
def state(project):
    # Use a non-trigger state so creating the issue doesn't also fire the
    # orchestration signal hook (which would try to render a prompt before the
    # seed fixture has run).
    return State.objects.create(name="Todo", project=project, group="unstarted")


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="Make button blue",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        priority="high",
    )


@pytest.fixture
def run(db, workspace, create_user, issue):
    return AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="", work_item=issue
    )


@pytest.mark.unit
def test_context_shape(issue, run):
    ctx = build_context(issue, run)
    assert ctx["issue"]["title"] == issue.name
    assert ctx["issue"]["priority"] == "high"
    assert ctx["issue"]["state"] == "Todo"
    assert ctx["issue"]["state_group"] == "unstarted"
    assert ctx["issue"]["identifier"].startswith("TP-")
    assert ctx["project"]["identifier"] == "TP"
    assert ctx["repo"]["url"] == "git@github.com:acme/web.git"
    assert ctx["repo"]["base_branch"] == "trunk"
    assert ctx["run"]["attempt"] == 1
    assert ctx["run"]["turn_number"] == 1


@pytest.mark.unit
def test_context_attempt_increments_on_follow_up(
    issue, run, workspace, create_user
):
    AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="prior", work_item=issue
    )
    ctx = build_context(issue, run)
    assert ctx["run"]["attempt"] == 2
