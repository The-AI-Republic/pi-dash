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
    # No git_work_branch set on the issue → should surface as None so templates
    # can branch on `{% if repo.work_branch %}` without false positives.
    assert ctx["repo"]["work_branch"] is None
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


@pytest.mark.unit
def test_context_includes_git_work_branch_when_set(issue, run):
    issue.git_work_branch = "feat/pinned-branch"
    issue.save(update_fields=["git_work_branch"])
    ctx = build_context(issue, run)
    assert ctx["repo"]["work_branch"] == "feat/pinned-branch"


@pytest.mark.unit
def test_context_parent_is_none_when_unset(issue, run):
    ctx = build_context(issue, run)
    assert ctx["parent"] is None


@pytest.mark.unit
def test_context_parent_uses_parents_own_project_identifier(
    workspace, project, state, create_user, run, issue
):
    # Parents may live in a different project than their child (the FK is just
    # a self-reference with no same-project constraint). The rendered parent
    # identifier must use the *parent's* project identifier, not the child's.
    other_project = Project.objects.create(
        name="Other Project",
        identifier="OP",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/other.git",
        base_branch="trunk",
    )
    other_state = State.objects.create(
        name="Todo", project=other_project, group="unstarted"
    )
    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=other_project,
        state=other_state,
        created_by=create_user,
        git_work_branch="pi-dash/op-1",
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"] is not None
    assert ctx["parent"]["identifier"].startswith("OP-"), (
        f"parent identifier should use parent's project (OP), got "
        f"{ctx['parent']['identifier']!r}"
    )
    assert ctx["parent"]["title"] == "Umbrella epic"
    assert ctx["parent"]["work_branch"] == "pi-dash/op-1"


@pytest.mark.unit
def test_context_parent_work_branch_empty_surfaces_as_none(
    workspace, project, state, create_user, run, issue
):
    parent = Issue.objects.create(
        name="Sibling parent",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        git_work_branch="",
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"] is not None
    assert ctx["parent"]["work_branch"] is None


@pytest.mark.unit
def test_context_empty_base_branch_surfaces_as_none(workspace, create_user):
    # A project with no base_branch set — empty strings must flow through as
    # ``None`` so the prompt template takes the "auto-detect remote default"
    # branch instead of rendering a literal empty string.
    project = Project.objects.create(
        name="No Default",
        identifier="ND",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/no-default.git",
        base_branch="",
    )
    project_state = State.objects.create(name="Todo", project=project, group="unstarted")
    issue = Issue.objects.create(
        name="Fix a thing",
        workspace=workspace,
        project=project,
        state=project_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="", work_item=issue
    )
    ctx = build_context(issue, run)
    assert ctx["repo"]["base_branch"] is None
    assert ctx["repo"]["work_branch"] is None
