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
def test_build_continuation_falls_back_to_first_turn_when_no_parent(
    seeded_default, issue, run
):
    """Without a parent run with a started_at, build_continuation should
    fall back to the full first-turn template rather than emitting a
    bare 'no new input' placeholder.
    """
    out = build_continuation(issue, run)
    # Same shape as build_first_turn — issue context, not just a
    # placeholder string.
    assert "Pi Dash issue" in out


# ---------------------------------------------------------------------------
# Phase-aware template selection
# (.ai_design/create_review_state/design.md §5 / §7.7)
# ---------------------------------------------------------------------------


@pytest.fixture
def seeded_review(db):
    from pi_dash.prompting.seed import seed_review_template

    seed_review_template(force=False)


@pytest.fixture
def in_review_state(project):
    return State.objects.create(
        name="In Review",
        project=project,
        group="review",
    )


@pytest.fixture
def in_progress_state(project):
    return State.objects.create(
        name="In Progress",
        project=project,
        group="started",
    )


@pytest.mark.unit
def test_build_first_turn_uses_review_template_for_in_review(
    db, seeded_default, seeded_review, workspace, project, in_review_state, create_user
):
    issue = Issue.objects.create(
        name="Some review work",
        workspace=workspace,
        project=project,
        state=in_review_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        work_item=issue,
    )
    rendered = build_first_turn(issue, run)
    # The review prompt's distinguishing language — the kind-router
    # is the load-bearing structural marker.
    assert "kind of review" in rendered.lower()
    assert "pr_url" in rendered or "design_doc_paths" in rendered


@pytest.mark.unit
def test_build_first_turn_uses_default_template_for_in_progress(
    db,
    seeded_default,
    seeded_review,
    workspace,
    project,
    in_progress_state,
    create_user,
):
    issue = Issue.objects.create(
        name="Some impl work",
        workspace=workspace,
        project=project,
        state=in_progress_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        work_item=issue,
    )
    rendered = build_first_turn(issue, run)
    # The default template's distinguishing marker.
    assert "Pi Dash issue" in rendered
    # Make sure we didn't accidentally render the review prompt for
    # an impl-phase issue.
    assert "kind of review" not in rendered.lower()
