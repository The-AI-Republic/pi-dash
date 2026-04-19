# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.urls import reverse

from pi_dash.db.models import Issue, Project, State
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def project(db, workspace, create_user):
    return Project.objects.create(
        name="Web", identifier="WEB", workspace=workspace, created_by=create_user
    )


@pytest.fixture
def state(project):
    # Non-trigger state so Issue creation doesn't create a real AgentRun via
    # the orchestration signal — the preview test is about rendering, not
    # delegation.
    return State.objects.create(name="Todo", project=project, group="unstarted")


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="Blue button",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )


@pytest.mark.contract
def test_preview_renders_template(seeded, session_client, workspace, issue):
    template = (
        PromptTemplate.objects.filter(workspace__isnull=True).first()
    )
    url = reverse(
        "prompting:prompt-template-preview",
        kwargs={"slug": workspace.slug, "template_id": template.id},
    )
    response = session_client.post(url, {"issue_id": str(issue.id)}, format="json")
    assert response.status_code == 200, response.content
    body = response.json()
    assert "prompt" in body
    assert issue.name in body["prompt"]
    # Preview must not create a run.
    assert AgentRun.objects.filter(work_item=issue).count() == 0


@pytest.mark.contract
def test_preview_requires_admin(seeded, api_client, workspace, issue, create_user):
    # Non-admin user
    from pi_dash.db.models import User

    other = User.objects.create(email="outsider@pi-dash.so")
    other.set_password("p"); other.save()
    api_client.force_authenticate(user=other)
    template = PromptTemplate.objects.filter(workspace__isnull=True).first()
    url = reverse(
        "prompting:prompt-template-preview",
        kwargs={"slug": workspace.slug, "template_id": template.id},
    )
    response = api_client.post(url, {"issue_id": str(issue.id)}, format="json")
    assert response.status_code == 403


@pytest.mark.contract
def test_preview_rejects_staff_non_member(seeded, api_client, workspace, issue):
    """is_staff alone must not grant preview access — only workspace-admin
    membership (role 20) or superuser do. Regression for a bypass that leaked
    preview access to any Django-admin user."""
    from pi_dash.db.models import User

    staff = User.objects.create(email="staff@pi-dash.so", is_staff=True)
    staff.set_password("p"); staff.save()
    api_client.force_authenticate(user=staff)
    template = PromptTemplate.objects.filter(workspace__isnull=True).first()
    url = reverse(
        "prompting:prompt-template-preview",
        kwargs={"slug": workspace.slug, "template_id": template.id},
    )
    response = api_client.post(url, {"issue_id": str(issue.id)}, format="json")
    assert response.status_code == 403


@pytest.mark.contract
def test_preview_missing_issue_returns_400(seeded, session_client, workspace):
    template = PromptTemplate.objects.filter(workspace__isnull=True).first()
    url = reverse(
        "prompting:prompt-template-preview",
        kwargs={"slug": workspace.slug, "template_id": template.id},
    )
    response = session_client.post(url, {}, format="json")
    assert response.status_code == 400
