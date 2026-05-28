# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from datetime import timedelta
from uuid import uuid4

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import Project, ProjectMember
from pi_dash.runner.models import AgentRun, Pod


def _add_project_member(project, user, role=20):
    member, _ = ProjectMember.objects.get_or_create(
        project=project,
        member=user,
        defaults={"role": role},
    )
    return member


def _create_project(workspace, user, identifier):
    project = Project.objects.create(
        name=f"Project {identifier}",
        identifier=identifier,
        workspace=workspace,
        created_by=user,
    )
    _add_project_member(project, user)
    return project


@pytest.mark.unit
def test_overview_includes_agent_run_token_totals(db, session_client, workspace, project, create_user):
    _add_project_member(project, create_user)
    pod = Pod.default_for_project(project)
    other_project = _create_project(workspace, create_user, f"P{uuid4().hex[:4].upper()}")
    other_pod = Pod.default_for_project(other_project)

    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        input_tokens=100,
        output_tokens=40,
        total_tokens=140,
    )
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        input_tokens=None,
        output_tokens=10,
        total_tokens=None,
    )
    AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=other_pod,
        input_tokens=900,
        output_tokens=90,
        total_tokens=990,
    )

    resp = session_client.get(
        f"/api/workspaces/{workspace.slug}/advance-analytics/",
        {"tab": "overview", "project_ids": str(project.id)},
    )

    assert resp.status_code == status.HTTP_200_OK
    assert resp.data["agent_run_input_tokens"] == {"count": 100}
    assert resp.data["agent_run_output_tokens"] == {"count": 50}
    assert resp.data["agent_run_total_tokens"] == {"count": 140}


@pytest.mark.unit
def test_overview_agent_run_tokens_respect_date_filter(db, session_client, workspace, project, create_user):
    _add_project_member(project, create_user)
    pod = Pod.default_for_project(project)

    current_run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        input_tokens=100,
        output_tokens=40,
        total_tokens=140,
    )
    old_run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        input_tokens=900,
        output_tokens=90,
        total_tokens=990,
    )
    AgentRun.objects.filter(pk=old_run.pk).update(created_at=timezone.now() - timedelta(days=40))

    resp = session_client.get(
        f"/api/workspaces/{workspace.slug}/advance-analytics/",
        {"tab": "overview", "date_filter": "last_7_days"},
    )

    assert resp.status_code == status.HTTP_200_OK
    assert resp.data["agent_run_input_tokens"] == {"count": current_run.input_tokens}
    assert resp.data["agent_run_output_tokens"] == {"count": current_run.output_tokens}
    assert resp.data["agent_run_total_tokens"] == {"count": current_run.total_tokens}
