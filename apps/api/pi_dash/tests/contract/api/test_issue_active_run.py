# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The ``active_run`` block on v1 single-issue reads (PDASHOSS01-407).

``pidash issue busy`` reads it to tell an outside agent that a run owns
the issue's branch, so it comments instead of pushing. The block matches
the Rust API (NEWPIDASH-251 + PDASHOSS01-407) key for key; it is absent
when no run occupies the issue's one-active-run slot.
"""

from __future__ import annotations

from datetime import datetime, timezone as dt_timezone
from unittest import mock

import pytest
from rest_framework import status

from pi_dash.db.models import ProjectMember
from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus

ACTIVE_RUN_KEYS = [
    "id",
    "status",
    "runner_id",
    "created_at",
    "assigned_at",
    "started_at",
    "age_seconds",
    "runner_last_heartbeat_at",
    "branch",
]


@pytest.fixture(autouse=True)
def _no_activity():
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def issue(workspace, project, create_user):
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    state = State.objects.create(name="Todo", group="unstarted", workspace=workspace, project=project)
    return Issue.objects.create(
        name="Busy branch",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )


@pytest.fixture
def runner(create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="laptop",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=datetime(2026, 11, 27, 16, 59, 30, tzinfo=dt_timezone.utc),
    )


def _run(issue, pod, user, run_status, runner=None, run_config=None):
    run = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=user,
        pod=pod,
        work_item=issue,
        status=run_status,
        runner=runner,
        run_config=run_config or {},
    )
    if runner is not None:
        AgentRun.objects.filter(pk=run.pk).update(
            assigned_at=datetime(2026, 11, 27, 16, 50, 0, 250000, tzinfo=dt_timezone.utc),
            started_at=datetime(2026, 11, 27, 16, 50, 5, tzinfo=dt_timezone.utc),
        )
        run.refresh_from_db()
    return run


def _detail(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/"


def _by_identifier(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/work-items/{issue.project.identifier}-{issue.sequence_id}/"


@pytest.mark.contract
@pytest.mark.django_db
class TestIssueActiveRun:
    def test_running_run_reports_run_and_branch(self, api_key_client, workspace, issue, pod, runner, create_user):
        issue.git_work_branch = "pi-dash/def-1"
        issue.save(update_fields=["git_work_branch"])
        run = _run(issue, pod, create_user, AgentRunStatus.RUNNING, runner=runner)

        for url in (_detail(workspace, issue), _by_identifier(workspace, issue)):
            resp = api_key_client.get(url)
            assert resp.status_code == status.HTTP_200_OK, resp.content
            block = resp.json()["active_run"]
            assert list(block) == ACTIVE_RUN_KEYS
            assert block["id"] == str(run.id)
            assert block["status"] == "running"
            assert block["runner_id"] == str(runner.id)
            assert block["created_at"] == run.created_at.astimezone(dt_timezone.utc).isoformat()
            assert block["assigned_at"] == "2026-11-27T16:50:00.250000+00:00"
            assert block["started_at"] == "2026-11-27T16:50:05+00:00"
            assert isinstance(block["age_seconds"], int) and block["age_seconds"] >= 0
            assert block["runner_last_heartbeat_at"] == "2026-11-27T16:59:30+00:00"
            assert block["branch"] == "pi-dash/def-1"

    def test_branch_falls_back_to_the_run_pin(self, api_key_client, workspace, issue, pod, create_user):
        _run(issue, pod, create_user, AgentRunStatus.QUEUED, run_config={"git_work_branch": " feat/pinned "})

        block = api_key_client.get(_detail(workspace, issue)).json()["active_run"]
        assert block["status"] == "queued"
        assert block["runner_id"] is None
        assert block["assigned_at"] is None and block["started_at"] is None
        assert block["runner_last_heartbeat_at"] is None
        assert block["branch"] == "feat/pinned"

    def test_no_branch_known_is_null(self, api_key_client, workspace, issue, pod, create_user):
        _run(issue, pod, create_user, AgentRunStatus.QUEUED, run_config={"git_work_branch": None})

        assert api_key_client.get(_detail(workspace, issue)).json()["active_run"]["branch"] is None

    def test_idle_issue_omits_the_block(self, api_key_client, workspace, issue, pod, create_user):
        issue.git_work_branch = "pi-dash/def-1"
        issue.save(update_fields=["git_work_branch"])
        _run(issue, pod, create_user, AgentRunStatus.COMPLETED)

        assert "active_run" not in api_key_client.get(_detail(workspace, issue)).json()

    def test_fields_gate_and_list_rows(self, api_key_client, workspace, issue, pod, runner, create_user):
        _run(issue, pod, create_user, AgentRunStatus.RUNNING, runner=runner)

        assert "active_run" not in api_key_client.get(_detail(workspace, issue) + "?fields=id,name").json()
        only = api_key_client.get(_detail(workspace, issue) + "?fields=active_run").json()
        assert only["active_run"]["status"] == "running"
        rows = api_key_client.get(
            f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/"
        ).json()["results"]
        assert rows and all("active_run" not in row for row in rows)
