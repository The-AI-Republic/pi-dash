# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``POST /api/v1/workspaces/<slug>/agent-runs/<run_id>/release-pin/``.

The token-facing recovery path for a stuck pin — ``pidash run release-pin
<run>``. Before it existed, a run pinned to a runner that would not free up
could only be rescued from the DB or a browser session: the ticker skips the
issue (a QUEUED run counts as active), ``run-ai`` answers 409
``active_run_exists``, and the CLI had no verb for it. See PDASHOSS01-272.
"""

from __future__ import annotations

import uuid

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    Project,
    ProjectMember,
    State,
    User,
    Workspace,
    WorkspaceMember,
)
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus


@pytest.fixture
def second_workspace(db, create_user):
    ws = Workspace.objects.create(name="OtherWS", owner=create_user, slug="other-ws-release-pin")
    WorkspaceMember.objects.create(workspace=ws, member=create_user, role=20)
    return ws


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Pins",
            identifier="PIN",
            workspace=workspace,
            created_by=create_user,
        )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def state(project, workspace, create_user):
    with impersonate(create_user):
        return State.objects.create(
            name="In Progress", project=project, workspace=workspace, group="started", default=True
        )


@pytest.fixture
def issue(db, workspace, project, state, create_user):
    with impersonate(create_user):
        return Issue.objects.create(
            name="Task", workspace=workspace, project=project, state=state, created_by=create_user
        )


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def busy_runner(workspace, pod, create_user):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="busy-one",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def parent_run(issue, pod, busy_runner, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        runner=busy_runner,
        thread_id="thread-abc",
        # Terminal: a follow-up only exists once its parent has stopped
        # (``agent_run_one_active_per_work_item`` enforces that), but the
        # stale ``thread_id`` resume handle is still on the row.
        status=AgentRunStatus.COMPLETED,
        prompt="the long one",
        started_at=timezone.now() - timezone.timedelta(minutes=10),
        ended_at=timezone.now(),
    )


@pytest.fixture
def pinned_run(issue, pod, busy_runner, parent_run, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        parent_run=parent_run,
        pinned_runner=busy_runner,
        status=AgentRunStatus.QUEUED,
        prompt="the stuck follow-up",
    )


def _url(workspace, run_id):
    return f"/api/v1/workspaces/{workspace.slug}/agent-runs/{run_id}/release-pin/"


@pytest.mark.unit
def test_release_pin_clears_the_pin_and_the_parents_thread_id(
    api_key_client, workspace, issue, pinned_run, parent_run, busy_runner
):
    resp = api_key_client.post(_url(workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["ok"] is True
    assert resp.data["run_id"] == str(pinned_run.id)
    assert resp.data["work_item_id"] == str(issue.id)
    assert resp.data["previous_pinned_runner_id"] == str(busy_runner.id)

    pinned_run.refresh_from_db()
    parent_run.refresh_from_db()
    assert pinned_run.pinned_runner_id is None
    assert pinned_run.status == AgentRunStatus.QUEUED
    # Same DB state as the web hatch and as the automatic over-budget
    # release: both go through ``matcher.clear_run_pin``.
    assert parent_run.thread_id == ""


@pytest.mark.unit
def test_release_pin_is_409_when_the_run_is_not_queued(api_key_client, workspace, pinned_run):
    AgentRun.objects.filter(pk=pinned_run.pk).update(status=AgentRunStatus.RUNNING)
    resp = api_key_client.post(_url(workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["code"] == "not_queued"
    pinned_run.refresh_from_db()
    assert pinned_run.pinned_runner_id is not None


@pytest.mark.unit
def test_release_pin_is_409_when_the_run_is_not_pinned(api_key_client, workspace, pinned_run):
    AgentRun.objects.filter(pk=pinned_run.pk).update(pinned_runner=None)
    resp = api_key_client.post(_url(workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["code"] == "not_pinned"


@pytest.mark.unit
def test_release_pin_is_409_for_a_cloud_agent_run(api_key_client, workspace, pinned_run):
    # ``agent_run_cloud_has_no_local_assignment`` forbids a cloud run holding
    # a pin at all, which is exactly why the endpoint answers before it looks
    # at the pin.
    AgentRun.objects.filter(pk=pinned_run.pk).update(
        executor_kind="cloud_agent", pinned_runner=None, owner=None
    )
    resp = api_key_client.post(_url(workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT, resp.data
    assert resp.data["code"] == "executor_not_local"


@pytest.mark.unit
def test_release_pin_is_404_for_an_unknown_run(api_key_client, workspace):
    resp = api_key_client.post(_url(workspace, uuid.uuid4()), {}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_release_pin_is_404_for_a_run_in_another_workspace(
    api_key_client, workspace, second_workspace, pinned_run
):
    """The slug in the URL scopes the lookup; membership elsewhere is no help."""
    resp = api_key_client.post(_url(second_workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_release_pin_is_404_for_a_member_with_no_claim_on_the_run(
    api_client, workspace, pinned_run, db
):
    """Workspace membership alone is not authority over somebody else's run —
    the same gate the web hatch uses (``_can_view_run``)."""
    unique = uuid.uuid4().hex[:8]
    bystander = User.objects.create(
        email=f"bystander-{unique}@example.com", username=f"bystander_{unique}"
    )
    WorkspaceMember.objects.create(workspace=workspace, member=bystander, role=15)
    api_client.force_authenticate(user=bystander)
    resp = api_client.post(_url(workspace, pinned_run.id), {}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND
    pinned_run.refresh_from_db()
    assert pinned_run.pinned_runner_id is not None
