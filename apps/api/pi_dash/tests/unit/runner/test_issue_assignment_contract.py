# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""HTTP contract for the dedicated runner on both surfaces (design §18).

``Contract, both surfaces`` row: the same set/change/clear over HTTP on
``/api/v1/workspaces/{slug}/projects/{id}/work-items/{id}/`` plus the
run-AI refusal codes on both the v1 token endpoint and the web endpoint.
The app-tier issue PATCH shares the same serializer + service, so it is
covered by the serializer tests; these cover the HTTP envelope.
"""

from __future__ import annotations

from unittest import mock

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import ProjectMember
from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import Pod, Runner, RunnerStatus


@pytest.fixture(autouse=True)
def _stub_transports():
    with mock.patch("pi_dash.runner.services.pubsub.send_to_runner"), mock.patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ), mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"), mock.patch(
        "pi_dash.bgtasks.webhook_task.model_activity.delay"
    ):
        yield


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def state(workspace, project):
    return State.objects.create(
        name="Backlog", group="backlog", workspace=workspace, project=project,
        sequence=1.0, default=True,
    )


def _make_runner(user, workspace, pod, name="gpu-box"):
    return Runner.objects.create(
        owner=user, workspace=workspace, pod=pod, name=name,
        status=RunnerStatus.ONLINE, last_heartbeat_at=timezone.now(),
    )


def _make_issue(workspace, project, state, user, *, runner=None, pod=None):
    from crum import impersonate

    with impersonate(user):
        return Issue.objects.create(
            name="An issue", project=project, workspace=workspace,
            state=state, created_by=user, assigned_pod=pod,
            assigned_runner=runner,
        )


def _v1_url(workspace, project, issue):
    return (
        f"/api/v1/workspaces/{workspace.slug}/projects/{project.id}"
        f"/work-items/{issue.id}/"
    )


@pytest.mark.unit
def test_v1_patch_set_change_clear(db, api_key_client, workspace, project, create_user, pod, state):
    """v1 PATCH set/change/clear over HTTP (§18 both surfaces)."""
    Issue.objects.all().delete()
    runner1 = _make_runner(create_user, workspace, pod, name="box-one")
    runner2 = _make_runner(create_user, workspace, pod, name="box-two")
    issue = _make_issue(workspace, project, state, create_user, pod=pod)
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})

    url = _v1_url(workspace, project, issue)
    resp = api_key_client.patch(url, {"assigned_runner": str(runner1.id)}, format="json")
    assert resp.status_code in (status.HTTP_200_OK, status.HTTP_204_NO_CONTENT), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner1.id

    resp = api_key_client.patch(url, {"assigned_runner": str(runner2.id)}, format="json")
    assert resp.status_code in (status.HTTP_200_OK, status.HTTP_204_NO_CONTENT), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner2.id

    resp = api_key_client.patch(url, {"assigned_runner": None}, format="json")
    assert resp.status_code in (status.HTTP_200_OK, status.HTTP_204_NO_CONTENT), resp.content
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_v1_run_ai_refuses_offline_dedicated(db, api_key_client, workspace, project, create_user, pod, state):
    """v1 run-ai: dedicated offline → 409 assigned_runner_offline, no run."""
    from pi_dash.runner.models import AgentRun

    runner = _make_runner(create_user, workspace, pod)
    runner.status = RunnerStatus.OFFLINE
    runner.last_heartbeat_at = None
    runner.save(update_fields=["status", "last_heartbeat_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})

    url = _v1_url(workspace, project, issue) + "run-ai/"
    resp = api_key_client.post(url)
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["reason"] == "assigned_runner_offline"
    assert AgentRun.objects.filter(work_item=issue).count() == 0
    issue.refresh_from_db()
    assert issue.assigned_runner_id == runner.id  # untouched


@pytest.mark.unit
def test_web_run_ai_refuses_offline_dedicated(db, session_client, workspace, project, create_user, pod, state):
    """web run_ai: dedicated offline → 409 full refusal body, no run (§23.3, §11.2)."""
    from pi_dash.runner.models import AgentRun

    runner = _make_runner(create_user, workspace, pod)
    runner.status = RunnerStatus.OFFLINE
    runner.last_heartbeat_at = None
    runner.save(update_fields=["status", "last_heartbeat_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)

    resp = session_client.post(
        "/api/runners/runs/", {"work_item": str(issue.id), "triggered_by": "run_ai"}, format="json"
    )
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["reason"] == "assigned_runner_offline"
    assert resp.data["runner"] == str(runner.id)
    assert resp.data["runner_name"] == runner.name
    assert resp.data["pod"] == str(pod.id)
    assert resp.data["options"] == ["pod_once", "change_runner"]
    assert AgentRun.objects.filter(work_item=issue).count() == 0
