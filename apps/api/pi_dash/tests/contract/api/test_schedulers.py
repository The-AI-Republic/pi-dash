# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the external (`/api/v1/`) read-only scheduler surface.

PDASHOSS01-225: a project member can list the schedulers of a project, read
one scheduler's full definition, and audit its recent runs; a non-member
gets the normal permission error. The surface is read-only — writes 405.
"""

from datetime import timedelta

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    APIToken,
    Issue,
    IssueComment,
    Project,
    ProjectMember,
    Scheduler,
    SchedulerBinding,
    User,
    WorkspaceMember,
)
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod


@pytest.fixture
def sched_project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Scheduler Test Project",
            identifier="SCH",
            workspace=workspace,
            created_by=create_user,
        )
    ProjectMember.objects.get_or_create(
        project=project,
        member=create_user,
        defaults={"role": 20, "is_active": True},
    )
    return project


@pytest.fixture
def scheduler(db, workspace, create_user):
    # Slug must not collide with the builtins seeded by the
    # post_save(Workspace) receiver in pi_dash.scheduler.signals.
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="contract-test-scheduler",
            name="Contract Test Scheduler",
            description="Reads the project and files issues.",
            prompt="Scan the project for stalled issues.",
        )


@pytest.fixture
def binding(db, scheduler, sched_project, workspace, create_user):
    with impersonate(create_user):
        return SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=sched_project,
            workspace=workspace,
            dtstart=timezone.now() - timedelta(days=1),
            rrule="FREQ=HOURLY",
            tzid="UTC",
            enabled=True,
            next_run_at=timezone.now() + timedelta(hours=1),
            actor=create_user,
        )


@pytest.fixture
def outside_user(db):
    """A user with no membership anywhere in the workspace."""
    user = User.objects.create(email="outsider@pi-dash.test", username="sched-outsider")
    user.set_password("test-password-123")
    user.save()
    return user


@pytest.fixture
def outside_client(db, outside_user):
    token = APIToken.objects.create(
        user=outside_user,
        label="Outsider Token",
        token="outsider-api-token-12345",
    )
    client = APIClient()
    client.credentials(HTTP_X_API_KEY=token.token)
    return client


@pytest.fixture
def nonmember_user(db, workspace):
    """A workspace member who is NOT a member of ``sched_project``."""
    user = User.objects.create(email="nonmember@pi-dash.test", username="sched-nonmember")
    user.set_password("test-password-123")
    user.save()
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=15)
    return user


@pytest.fixture
def nonmember_client(db, nonmember_user):
    token = APIToken.objects.create(
        user=nonmember_user,
        label="Non-member Token",
        token="nonmember-api-token-12345",
    )
    client = APIClient()
    client.credentials(HTTP_X_API_KEY=token.token)
    return client


def _make_run(binding, user, *, status=AgentRunStatus.QUEUED, **kwargs):
    return AgentRun.objects.create(
        workspace=binding.workspace,
        created_by=user,
        pod=Pod.default_for_project_id(binding.project_id),
        scheduler_binding=binding,
        status=status,
        prompt="scheduled scan",
        **kwargs,
    )


def _ws_url(slug):
    return f"/api/v1/workspaces/{slug}/schedulers/"


def _list_url(slug, project_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/"


def _detail_url(slug, project_id, scheduler_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/"


def _runs_url(slug, project_id, scheduler_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/runs/"


@pytest.mark.contract
class TestWorkspaceSchedulerList:
    @pytest.mark.django_db
    def test_member_lists_workspace_schedulers(
        self, api_key_client, workspace, scheduler, binding, sched_project
    ):
        response = api_key_client.get(_ws_url(workspace.slug))
        assert response.status_code == http_status.HTTP_200_OK
        rows = {row["slug"]: row for row in response.data["results"]}
        assert "contract-test-scheduler" in rows
        row = rows["contract-test-scheduler"]
        assert row["prompt"] == "Scan the project for stalled issues."
        assert row["is_enabled"] is True
        binding_projects = [b["project"] for b in row["bindings"]]
        assert str(sched_project.id) in [str(p) for p in binding_projects]

    @pytest.mark.django_db
    def test_bindings_hidden_for_non_project_members(
        self, nonmember_client, workspace, scheduler, binding
    ):
        # A workspace member who is not on the project still sees the
        # definition, but not the install on a project they cannot access.
        response = nonmember_client.get(_ws_url(workspace.slug))
        assert response.status_code == http_status.HTTP_200_OK
        rows = {row["slug"]: row for row in response.data["results"]}
        assert rows["contract-test-scheduler"]["bindings"] == []

    @pytest.mark.django_db
    def test_outsider_denied(self, outside_client, workspace, scheduler):
        response = outside_client.get(_ws_url(workspace.slug))
        assert response.status_code == http_status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_write_methods_rejected(self, api_key_client, workspace, scheduler):
        response = api_key_client.post(_ws_url(workspace.slug), {}, format="json")
        assert response.status_code == http_status.HTTP_405_METHOD_NOT_ALLOWED


@pytest.mark.contract
class TestProjectSchedulerList:
    @pytest.mark.django_db
    def test_member_lists_project_schedulers(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        response = api_key_client.get(_list_url(workspace.slug, sched_project.id))
        assert response.status_code == http_status.HTTP_200_OK
        rows = response.data["results"]
        assert len(rows) == 1
        row = rows[0]
        assert row["slug"] == "contract-test-scheduler"
        assert row["name"] == "Contract Test Scheduler"
        assert len(row["bindings"]) == 1
        b = row["bindings"][0]
        assert b["rrule"] == "FREQ=HOURLY"
        assert b["enabled"] is True
        assert b["next_run_at"] is not None
        assert b["outcome_mode"] == "create_issue"

    @pytest.mark.django_db
    def test_uninstalled_scheduler_not_listed(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        # No binding fixture — the definition exists but is not installed
        # on this project.
        response = api_key_client.get(_list_url(workspace.slug, sched_project.id))
        assert response.status_code == http_status.HTTP_200_OK
        slugs = [row["slug"] for row in response.data["results"]]
        assert "contract-test-scheduler" not in slugs

    @pytest.mark.django_db
    def test_nonmember_denied(
        self, nonmember_client, workspace, sched_project, scheduler, binding
    ):
        response = nonmember_client.get(_list_url(workspace.slug, sched_project.id))
        assert response.status_code == http_status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestProjectSchedulerDetail:
    @pytest.mark.django_db
    def test_member_reads_full_definition(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        response = api_key_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["prompt"] == "Scan the project for stalled issues."
        assert response.data["source"] == "builtin"
        assert len(response.data["bindings"]) == 1
        assert str(response.data["bindings"][0]["id"]) == str(binding.id)

    @pytest.mark.django_db
    def test_404_when_not_installed_on_project(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        response = api_key_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    @pytest.mark.django_db
    def test_nonmember_denied(
        self, nonmember_client, workspace, sched_project, scheduler, binding
    ):
        response = nonmember_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestProjectSchedulerRuns:
    @pytest.mark.django_db
    def test_member_lists_runs_with_touched_issues(
        self, api_key_client, workspace, sched_project, scheduler, binding, create_user
    ):
        older = _make_run(
            binding,
            create_user,
            status=AgentRunStatus.FAILED,
            error_code="watchdog_timeout",
        )
        newer = _make_run(binding, create_user)
        with impersonate(create_user):
            issue = Issue.objects.create(
                name="Filed by scheduler",
                project=sched_project,
                workspace=workspace,
                created_by=create_user,
            )
            IssueComment.objects.create(
                issue=issue,
                workspace=workspace,
                project=sched_project,
                actor=create_user,
                comment_html="<p>Filed a finding</p>",
                speaker_agent_run_id=older.id,
            )

        response = api_key_client.get(
            _runs_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_200_OK
        rows = response.data["results"]
        assert [row["id"] for row in rows] == [str(newer.id), str(older.id)]
        assert rows[1]["status"] == "failed"
        assert rows[1]["error_code"] == "watchdog_timeout"
        touched = rows[1]["issues"]
        assert len(touched) == 1
        assert touched[0]["identifier"] == f"SCH-{issue.sequence_id}"
        assert touched[0]["name"] == "Filed by scheduler"
        assert rows[0]["issues"] == []

    @pytest.mark.django_db
    def test_per_page_bounds_the_page(
        self, api_key_client, workspace, sched_project, scheduler, binding, create_user
    ):
        for _ in range(3):
            _make_run(binding, create_user)
        response = api_key_client.get(
            _runs_url(workspace.slug, sched_project.id, scheduler.id),
            {"per_page": 2},
        )
        assert response.status_code == http_status.HTTP_200_OK
        assert len(response.data["results"]) == 2
        assert response.data["total_count"] == 3

    @pytest.mark.django_db
    def test_404_when_not_installed_on_project(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        response = api_key_client.get(
            _runs_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    @pytest.mark.django_db
    def test_nonmember_denied(
        self, nonmember_client, workspace, sched_project, scheduler, binding
    ):
        response = nonmember_client.get(
            _runs_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_403_FORBIDDEN
