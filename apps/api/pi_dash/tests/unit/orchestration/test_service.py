# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from unittest import mock

import pytest
from crum import impersonate

from pi_dash.db.models import Issue, Project, State
from pi_dash.orchestration import service
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def project(db, workspace, create_user):
    # ``BaseModel.save`` pulls ``created_by`` from ``crum.get_current_user()``
    # and wipes any direct FK assignment. Tests run without a request, so we
    # impersonate the user explicitly to populate the audit fields.
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )


@pytest.fixture
def states(project, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project, group="unstarted"
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, group="started"
            ),
            "done": State.objects.create(
                name="Done", project=project, group="completed"
            ),
        }


@pytest.fixture
def issue(workspace, project, states, create_user):
    with impersonate(create_user):
        return Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )


@pytest.fixture(autouse=True)
def no_runner_dispatch(monkeypatch):
    """Every test in this file exercises orchestration logic, not the runner
    fan-out. Stub the dispatcher so tests don't need a Redis."""
    monkeypatch.setattr(service, "_dispatch_to_runner", mock.Mock())
    return service._dispatch_to_runner


@pytest.mark.unit
def test_todo_to_in_progress_creates_run(seeded, issue, states):
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    assert outcome.created_run is not None
    assert outcome.created_run.status == AgentRunStatus.QUEUED
    assert outcome.created_run.parent_run_id is None
    assert "Pi Dash issue" in outcome.created_run.prompt


@pytest.mark.unit
def test_no_op_when_active_run_exists(seeded, issue, states, workspace, create_user):
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="x",
        work_item=issue,
        status=AgentRunStatus.RUNNING,
    )
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "active-run-exists"
    assert outcome.created_run is None


@pytest.mark.unit
def test_follow_up_run_links_parent(
    seeded, issue, states, workspace, create_user
):
    prior = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="first attempt",
        work_item=issue,
        status=AgentRunStatus.BLOCKED,
    )
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    assert outcome.created_run.parent_run_id == prior.id


@pytest.mark.unit
def test_non_trigger_state_does_nothing(seeded, issue, states):
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["done"],
    )
    assert outcome.reason == "not-a-trigger-state"
    assert outcome.created_run is None
