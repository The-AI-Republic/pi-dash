# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import Issue, Project, State, Workspace
from pi_dash.prompting.composer import (
    PromptTemplateNotFound,
    build_continuation,
    build_first_turn,
    load_template,
)
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun


@pytest.fixture
def seeded_default(db):
    seed_default_template(force=False)


@pytest.fixture
def project(db, workspace, create_user):
    return Project.objects.create(
        name="Test Project",
        identifier="TP",
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture
def state(project):
    # Use a non-trigger state so Issue creation doesn't also fire the
    # orchestration signal and race with the explicit fixtures below.
    return State.objects.create(
        name="Todo",
        project=project,
        group="unstarted",
    )


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="Wire up the blue button",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        priority="medium",
    )


@pytest.fixture
def run(db, workspace, create_user, issue):
    return AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        work_item=issue,
    )


@pytest.mark.unit
def test_load_template_falls_back_to_global(db, seeded_default, workspace):
    template = load_template(workspace)
    assert template.workspace_id is None
    assert PromptTemplate.DEFAULT_NAME in template.name


@pytest.mark.unit
def test_load_template_prefers_workspace_override(db, seeded_default, workspace):
    override = PromptTemplate.objects.create(
        workspace=workspace,
        name=PromptTemplate.DEFAULT_NAME,
        body="custom body for {{ issue.identifier }}",
    )
    chosen = load_template(workspace)
    assert chosen.id == override.id


@pytest.mark.unit
def test_load_template_raises_when_unseeded(db, workspace):
    PromptTemplate.objects.all().delete()
    with pytest.raises(PromptTemplateNotFound):
        load_template(workspace)


@pytest.mark.unit
def test_build_first_turn_uses_default_template(seeded_default, issue, run):
    rendered = build_first_turn(issue, run)
    assert "Pi Dash issue" in rendered
    assert issue.name in rendered
    assert "TP-" in rendered  # project identifier-based issue identifier


@pytest.mark.unit
def test_build_continuation_not_implemented(seeded_default, issue, run):
    with pytest.raises(NotImplementedError):
        build_continuation(issue, run)
