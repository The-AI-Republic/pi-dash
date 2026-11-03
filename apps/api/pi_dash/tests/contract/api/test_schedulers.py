# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the external (`/api/v1/`) scheduler surface.

PDASHOSS01-225: a project member can list the schedulers of a project, read
one scheduler's full definition, and audit its recent runs; a non-member
gets the normal permission error.

PDASHOSS01-278: a project admin can edit a scheduler's install on the
project (the binding) and make it run now. Members, guests and non-members
get 403. A workspace admin can edit a user-created definition's prompt, name
and description; built-in definitions are rejected.

PDASHOSS01-318: a workspace admin can create and delete a definition, and a
project admin can install one on a project and uninstall it. Built-in
definitions can be installed and uninstalled but not deleted.
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
    Workspace,
    WorkspaceMember,
)
from pi_dash.db.models.scheduler import SchedulerSource
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
    @pytest.mark.parametrize("method", ["put", "patch", "delete"])
    def test_other_write_methods_rejected(
        self, api_key_client, workspace, scheduler, method
    ):
        response = getattr(api_key_client, method)(
            _ws_url(workspace.slug), {}, format="json"
        )
        assert response.status_code == http_status.HTTP_405_METHOD_NOT_ALLOWED

    @pytest.mark.django_db
    def test_builtins_flagged(self, api_key_client, workspace, scheduler):
        response = api_key_client.get(_ws_url(workspace.slug))

        assert response.status_code == http_status.HTTP_200_OK
        flags = {row["slug"]: row["is_builtin"] for row in response.data["results"]}
        # ``source`` reads ``builtin`` for both (PDASHOSS01-290); is_builtin
        # tells them apart.
        assert flags["contract-test-scheduler"] is False
        assert flags["security-audit"] is True


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
            error_details={"code": "watchdog_timeout"},
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
    def test_issue_attribution_scoped_to_callers_projects(
        self,
        api_key_client,
        workspace,
        sched_project,
        scheduler,
        binding,
        create_user,
        nonmember_user,
    ):
        # A run that commented on an issue in a project the caller is not a
        # member of must not leak that issue's identifier/name through the
        # audit trail — same visibility rule as the workspace-list bindings.
        run = _make_run(binding, create_user)
        with impersonate(nonmember_user):
            hidden_project = Project.objects.create(
                name="Hidden Project",
                identifier="HID",
                workspace=workspace,
                created_by=nonmember_user,
            )
            hidden_issue = Issue.objects.create(
                name="Should not be visible",
                project=hidden_project,
                workspace=workspace,
                created_by=nonmember_user,
            )
            IssueComment.objects.create(
                issue=hidden_issue,
                workspace=workspace,
                project=hidden_project,
                actor=nonmember_user,
                comment_html="<p>Cross-project comment</p>",
                speaker_agent_run_id=run.id,
            )
        with impersonate(create_user):
            visible_issue = Issue.objects.create(
                name="Visible to caller",
                project=sched_project,
                workspace=workspace,
                created_by=create_user,
            )
            IssueComment.objects.create(
                issue=visible_issue,
                workspace=workspace,
                project=sched_project,
                actor=create_user,
                comment_html="<p>In-project comment</p>",
                speaker_agent_run_id=run.id,
            )

        response = api_key_client.get(
            _runs_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_200_OK
        (row,) = response.data["results"]
        touched = {issue["name"] for issue in row["issues"]}
        assert touched == {"Visible to caller"}

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


def _binding_url(slug, project_id, scheduler_id):
    return f"{_detail_url(slug, project_id, scheduler_id)}binding/"


def _run_now_url(slug, project_id, scheduler_id):
    return f"{_detail_url(slug, project_id, scheduler_id)}run-now/"


def _project_client(workspace, project, *, role, label):
    """An API-key client for a fresh user holding ``role`` on the project."""
    user = User.objects.create(
        email=f"{label}@pi-dash.test", username=f"sched-{label}"
    )
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=role)
    ProjectMember.objects.create(
        project=project, member=user, role=role, is_active=True
    )
    token = APIToken.objects.create(
        user=user, label=f"{label} token", token=f"{label}-api-token-12345"
    )
    client = APIClient()
    client.credentials(HTTP_X_API_KEY=token.token)
    return client


@pytest.fixture
def member_client(db, workspace, sched_project):
    return _project_client(workspace, sched_project, role=15, label="member")


@pytest.fixture
def guest_client(db, workspace, sched_project):
    return _project_client(workspace, sched_project, role=5, label="guest")


@pytest.mark.contract
class TestProjectSchedulerBindingUpdate:
    @pytest.mark.django_db
    def test_admin_updates_extra_context(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        binding.extra_context = "Rule D: old wording."
        binding.save(update_fields=["extra_context"])
        new_text = "## Rules\n\nRule D: a queued run is not stuck."

        response = api_key_client.patch(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            {"extra_context": new_text},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["extra_context"] == new_text
        assert str(response.data["id"]) == str(binding.id)
        # The prior value comes back so a bad edit can be reverted.
        assert response.data["previous"] == {"extra_context": "Rule D: old wording."}

        # A later read shows the new text...
        detail = api_key_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert detail.data["bindings"][0]["extra_context"] == new_text
        # ...and so does the prompt the next scheduled run is dispatched with.
        from pi_dash.prompting.context import build_scheduler_task_body

        binding.refresh_from_db()
        assert "Rule D: a queued run is not stuck." in build_scheduler_task_body(
            binding
        )

    @pytest.mark.django_db
    def test_admin_toggles_enabled(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        url = _binding_url(workspace.slug, sched_project.id, scheduler.id)

        response = api_key_client.patch(url, {"enabled": False}, format="json")
        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["enabled"] is False
        assert response.data["previous"] == {"enabled": True}
        binding.refresh_from_db()
        assert binding.enabled is False

        response = api_key_client.patch(url, {"enabled": True}, format="json")
        assert response.status_code == http_status.HTTP_200_OK
        binding.refresh_from_db()
        assert binding.enabled is True

    @pytest.mark.django_db
    def test_cadence_change_recomputes_next_run_at(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        # Hourly, so due within the hour — a weekly rule must push it out.
        before = binding.next_run_at

        response = api_key_client.patch(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            {"rrule": "FREQ=WEEKLY"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["rrule"] == "FREQ=WEEKLY"
        binding.refresh_from_db()
        assert binding.next_run_at > before + timedelta(days=1)
        assert response.data["previous"]["rrule"] == "FREQ=HOURLY"
        assert response.data["previous"]["next_run_at"] is not None

    @pytest.mark.django_db
    def test_outcome_mode_and_pod(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        pod = Pod.default_for_project_id(sched_project.id)

        response = api_key_client.patch(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            {"outcome_mode": "apply_fix", "pod": str(pod.id)},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["outcome_mode"] == "apply_fix"
        assert str(response.data["pod"]) == str(pod.id)
        assert response.data["previous"] == {
            "outcome_mode": "create_issue",
            "pod": None,
        }

    @pytest.mark.django_db
    def test_app_serializer_validation_applies(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        # Same validation path as the web app: a bad RRULE and a pod from
        # another project are both field errors, and nothing is written.
        url = _binding_url(workspace.slug, sched_project.id, scheduler.id)

        response = api_key_client.patch(url, {"rrule": "FREQ=NEVER"}, format="json")
        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "rrule" in response.data

        foreign_pod = Pod.default_for_project_id(
            Project.objects.get(workspace=workspace, identifier="DEF").id
        )
        response = api_key_client.patch(
            url, {"pod": str(foreign_pod.id), "enabled": False}, format="json"
        )
        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "pod" in response.data
        binding.refresh_from_db()
        assert binding.rrule == "FREQ=HOURLY"
        assert binding.enabled is True

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "payload",
        [
            {"prompt": "Do something else entirely."},
            {"extra_context": "ok", "name": "Renamed"},
            {"next_run_at": "2030-01-01T00:00:00Z"},
            {"scheduler": "00000000-0000-0000-0000-000000000000"},
            {},
        ],
    )
    def test_non_binding_fields_rejected(
        self, api_key_client, workspace, sched_project, scheduler, binding, payload
    ):
        # The shared definition (prompt, name) and runtime state are not
        # writable through the binding; the request fails whole.
        response = api_key_client.patch(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            payload,
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "extra_context" in response.data["writable_fields"]
        scheduler.refresh_from_db()
        binding.refresh_from_db()
        assert scheduler.prompt == "Scan the project for stalled issues."
        assert scheduler.name == "Contract Test Scheduler"
        assert binding.extra_context == ""

    @pytest.mark.django_db
    @pytest.mark.parametrize("method", ["patch", "put", "post", "delete"])
    def test_project_detail_route_stays_get_only(
        self, api_key_client, workspace, sched_project, scheduler, binding, method
    ):
        # The definition is written through its workspace-scoped route.
        response = getattr(api_key_client, method)(
            _detail_url(workspace.slug, sched_project.id, scheduler.id),
            {"prompt": "Do something else entirely."},
            format="json",
        )
        assert response.status_code == http_status.HTTP_405_METHOD_NOT_ALLOWED
        scheduler.refresh_from_db()
        assert scheduler.prompt == "Scan the project for stalled issues."

    @pytest.mark.django_db
    def test_stale_token_is_precondition_failed(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        url = _binding_url(workspace.slug, sched_project.id, scheduler.id)
        read = api_key_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        token = read.data["bindings"][0]["updated_at"]

        # Fresh token, in the body: the write goes through.
        first = api_key_client.patch(
            url,
            {"extra_context": "first", "expected_updated_at": token},
            format="json",
        )
        assert first.status_code == http_status.HTTP_200_OK

        # The same token is now stale — as a body field and as If-Match.
        second = api_key_client.patch(
            url,
            {"extra_context": "second", "expected_updated_at": token},
            format="json",
        )
        assert second.status_code == http_status.HTTP_412_PRECONDITION_FAILED
        third = api_key_client.patch(
            url, {"extra_context": "third"}, format="json", HTTP_IF_MATCH=token
        )
        assert third.status_code == http_status.HTTP_412_PRECONDITION_FAILED
        binding.refresh_from_db()
        assert binding.extra_context == "first"

        # The 412 names the current value, which then works.
        fourth = api_key_client.patch(
            url,
            {"extra_context": "fourth"},
            format="json",
            HTTP_IF_MATCH=second.data["updated_at"],
        )
        assert fourth.status_code == http_status.HTTP_200_OK

        garbage = api_key_client.patch(
            url,
            {"extra_context": "x", "expected_updated_at": "yesterday"},
            format="json",
        )
        assert garbage.status_code == http_status.HTTP_400_BAD_REQUEST

        # Well-formed but out of range: still a 400, never a 500.
        out_of_range = api_key_client.patch(
            url,
            {"extra_context": "x"},
            format="json",
            HTTP_IF_MATCH="2026-13-01T00:00:00+00:00",
        )
        assert out_of_range.status_code == http_status.HTTP_400_BAD_REQUEST
        binding.refresh_from_db()
        assert binding.extra_context == "fourth"

    @pytest.mark.django_db
    def test_404_when_not_installed_on_project(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        response = api_key_client.patch(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            {"enabled": False},
            format="json",
        )
        assert response.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.contract
class TestProjectSchedulerRunNow:
    @pytest.mark.django_db
    def test_admin_run_now_makes_binding_due(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        from pi_dash.bgtasks.scheduler import _next_fire_for_binding

        assert binding.next_run_at > timezone.now()

        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["previous"]["next_run_at"] is not None
        binding.refresh_from_db()
        # Due: the scanner's `next_run_at <= now` filter picks it up on its
        # next tick...
        assert SchedulerBinding.objects.filter(
            pk=binding.pk, enabled=True, next_run_at__lte=timezone.now()
        ).exists()
        # ...and the fire path then advances it along the unchanged rule,
        # so the regular schedule resumes.
        assert binding.rrule == "FREQ=HOURLY"
        assert _next_fire_for_binding(binding, now=timezone.now()) > timezone.now()

    @pytest.mark.django_db
    def test_terminal_last_run_does_not_block(
        self, api_key_client, workspace, sched_project, scheduler, binding, create_user
    ):
        _make_run(binding, create_user, status=AgentRunStatus.COMPLETED)
        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )
        assert response.status_code == http_status.HTTP_200_OK

    @pytest.mark.django_db
    def test_409_when_binding_disabled(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        binding.enabled = False
        binding.save(update_fields=["enabled"])
        scheduled = binding.next_run_at

        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "binding_disabled"
        binding.refresh_from_db()
        assert binding.next_run_at == scheduled

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "run_status", [AgentRunStatus.QUEUED, AgentRunStatus.RUNNING]
    )
    def test_409_when_run_active(
        self,
        api_key_client,
        workspace,
        sched_project,
        scheduler,
        binding,
        create_user,
        run_status,
    ):
        _make_run(binding, create_user, status=run_status)
        scheduled = binding.next_run_at

        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "run_active"
        binding.refresh_from_db()
        assert binding.next_run_at == scheduled

    @pytest.mark.django_db
    def test_409_when_definition_disabled(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        scheduler.is_enabled = False
        scheduler.save(update_fields=["is_enabled"])
        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )
        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "scheduler_disabled"

    @pytest.mark.django_db
    def test_409_when_schedule_exhausted(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        # A single-shot whose moment has passed: firing it would only make
        # the scanner disable the binding, so run-now refuses instead.
        binding.rrule = ""
        binding.save(update_fields=["rrule"])
        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )
        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "no_future_occurrence"
        binding.refresh_from_db()
        assert binding.enabled is True

    @pytest.mark.django_db
    def test_404_when_not_installed_on_project(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        response = api_key_client.post(
            _run_now_url(workspace.slug, sched_project.id, scheduler.id),
            {},
            format="json",
        )
        assert response.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.contract
class TestSchedulerWritePermissions:
    """Every write route is project-admin only."""

    def _write(self, client, route, workspace, project, scheduler):
        if route == "binding":
            return client.patch(
                _binding_url(workspace.slug, project.id, scheduler.id),
                {"extra_context": "not allowed", "enabled": False},
                format="json",
            )
        return client.post(
            _run_now_url(workspace.slug, project.id, scheduler.id), {}, format="json"
        )

    @pytest.mark.django_db
    @pytest.mark.parametrize("route", ["binding", "run-now"])
    @pytest.mark.parametrize(
        "client_fixture",
        ["member_client", "guest_client", "nonmember_client", "outside_client"],
    )
    def test_non_admins_denied(
        self,
        request,
        workspace,
        sched_project,
        scheduler,
        binding,
        route,
        client_fixture,
    ):
        client = request.getfixturevalue(client_fixture)
        scheduled = binding.next_run_at

        response = self._write(client, route, workspace, sched_project, scheduler)

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        binding.refresh_from_db()
        assert binding.extra_context == ""
        assert binding.enabled is True
        assert binding.next_run_at == scheduled

    @pytest.mark.django_db
    @pytest.mark.parametrize("route", ["binding", "run-now"])
    def test_unauthenticated_denied(
        self, workspace, sched_project, scheduler, binding, route
    ):
        response = self._write(
            APIClient(), route, workspace, sched_project, scheduler
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        binding.refresh_from_db()
        assert binding.extra_context == ""

    @pytest.mark.django_db
    def test_members_can_still_read(
        self, member_client, workspace, sched_project, scheduler, binding
    ):
        response = member_client.get(
            _detail_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_200_OK


def _definition_url(slug, scheduler_id):
    return f"{_ws_url(slug)}{scheduler_id}/"


@pytest.fixture
def custom_scheduler(db, scheduler):
    """A user-created definition, as the web app stores it.

    ``Scheduler.source`` defaults to ``builtin`` even for user-created rows
    (PDASHOSS01-290). Built-ins are told apart by their catalog slug, so
    this row is editable with its default ``source``.
    """
    assert scheduler.source == SchedulerSource.BUILTIN
    return scheduler


@pytest.fixture
def project_admin_client(db, workspace, sched_project):
    """Admin of the project, plain member of the workspace."""
    client = _project_client(workspace, sched_project, role=15, label="projadmin")
    ProjectMember.objects.filter(
        project=sched_project, member__username="sched-projadmin"
    ).update(role=20)
    return client


@pytest.mark.contract
class TestWorkspaceSchedulerDefinitionUpdate:
    @pytest.mark.django_db
    def test_admin_replaces_prompt(
        self, api_key_client, workspace, sched_project, custom_scheduler, binding
    ):
        new_prompt = "## Coordinator\n\nA queued run is not stuck."

        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"prompt": new_prompt},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["prompt"] == new_prompt
        assert str(response.data["id"]) == str(custom_scheduler.id)
        # The prior value comes back so a bad edit can be reverted.
        assert response.data["previous"] == {
            "prompt": "Scan the project for stalled issues."
        }
        # The blast radius: every project the definition is installed on.
        assert response.data["affected_projects"] == [
            {
                "id": str(sched_project.id),
                "identifier": "SCH",
                "name": "Scheduler Test Project",
                "binding_enabled": True,
            }
        ]

        # A later read shows the new text...
        detail = api_key_client.get(
            _detail_url(workspace.slug, sched_project.id, custom_scheduler.id)
        )
        assert detail.data["prompt"] == new_prompt
        # ...and so does the prompt the next scheduled run is dispatched with.
        from pi_dash.prompting.context import build_scheduler_task_body

        binding.refresh_from_db()
        assert "A queued run is not stuck." in build_scheduler_task_body(binding)

    @pytest.mark.django_db
    def test_affected_projects_lists_every_live_install(
        self, api_key_client, workspace, sched_project, custom_scheduler, binding,
        create_user,
    ):
        # A project the caller is not a member of still counts, an
        # uninstalled (soft-deleted) binding does not.
        with impersonate(create_user):
            other = Project.objects.create(
                name="Another Project", identifier="OTH", workspace=workspace
            )
            gone = Project.objects.create(
                name="Uninstalled Project", identifier="GON", workspace=workspace
            )
            for project, enabled in ((other, False), (gone, True)):
                SchedulerBinding.objects.create(
                    scheduler=custom_scheduler,
                    project=project,
                    workspace=workspace,
                    dtstart=timezone.now() - timedelta(days=1),
                    rrule="FREQ=DAILY",
                    tzid="UTC",
                    enabled=enabled,
                )
        ProjectMember.objects.filter(project__in=[other, gone]).delete()
        SchedulerBinding.objects.filter(project=gone).update(
            deleted_at=timezone.now()
        )

        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"prompt": "New prompt."},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert [
            (p["identifier"], p["binding_enabled"])
            for p in response.data["affected_projects"]
        ] == [("OTH", False), ("SCH", True)]

    @pytest.mark.django_db
    def test_name_and_description(self, api_key_client, workspace, custom_scheduler):
        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"name": "Coordinator", "description": "Keeps the project moving."},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["name"] == "Coordinator"
        assert response.data["description"] == "Keeps the project moving."
        assert response.data["previous"] == {
            "name": "Contract Test Scheduler",
            "description": "Reads the project and files issues.",
        }
        # Not installed anywhere: nothing is affected.
        assert response.data["affected_projects"] == []
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "Scan the project for stalled issues."

    @pytest.mark.django_db
    def test_builtin_rejected(self, api_key_client, workspace):
        builtin = Scheduler.objects.get(workspace=workspace, slug="security-audit")
        assert builtin.source == SchedulerSource.BUILTIN
        seeded = builtin.prompt

        response = api_key_client.patch(
            _definition_url(workspace.slug, builtin.id),
            {"prompt": "Do something else entirely."},
            format="json",
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "builtin_scheduler"
        builtin.refresh_from_db()
        assert builtin.prompt == seeded

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "payload",
        [
            {"slug": "renamed"},
            {"prompt": "ok", "source": "manifest"},
            {"is_builtin": False},
            {"extra_context": "belongs on the binding"},
            {},
        ],
    )
    def test_other_fields_rejected(
        self, api_key_client, workspace, custom_scheduler, payload
    ):
        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            payload,
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data["writable_fields"] == [
            "prompt",
            "name",
            "description",
            "color",
            "is_enabled",
        ]
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "Scan the project for stalled issues."
        assert custom_scheduler.slug == "contract-test-scheduler"
        assert custom_scheduler.is_enabled is True

    @pytest.mark.django_db
    def test_app_serializer_validation_applies(
        self, api_key_client, workspace, custom_scheduler
    ):
        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"prompt": "   "},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "prompt" in response.data
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "Scan the project for stalled issues."

    @pytest.mark.django_db
    def test_stale_token_is_precondition_failed(
        self, api_key_client, workspace, custom_scheduler
    ):
        url = _definition_url(workspace.slug, custom_scheduler.id)
        token = custom_scheduler.updated_at.isoformat()

        first = api_key_client.patch(
            url, {"prompt": "first", "expected_updated_at": token}, format="json"
        )
        assert first.status_code == http_status.HTTP_200_OK

        second = api_key_client.patch(
            url, {"prompt": "second"}, format="json", HTTP_IF_MATCH=token
        )
        assert second.status_code == http_status.HTTP_412_PRECONDITION_FAILED
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "first"

        # The 412 names the current value, which then works.
        third = api_key_client.patch(
            url,
            {"prompt": "third"},
            format="json",
            HTTP_IF_MATCH=second.data["updated_at"],
        )
        assert third.status_code == http_status.HTTP_200_OK

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "client_fixture",
        [
            "project_admin_client",
            "member_client",
            "guest_client",
            "nonmember_client",
            "outside_client",
        ],
    )
    def test_non_workspace_admins_denied(
        self, request, workspace, custom_scheduler, binding, client_fixture
    ):
        client = request.getfixturevalue(client_fixture)

        response = client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"prompt": "not allowed"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "Scan the project for stalled issues."

    @pytest.mark.django_db
    def test_unauthenticated_denied(self, workspace, custom_scheduler):
        response = APIClient().patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"prompt": "not allowed"},
            format="json",
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.prompt == "Scan the project for stalled issues."

    @pytest.mark.django_db
    def test_404_for_another_workspaces_scheduler(
        self, api_key_client, workspace, create_user
    ):
        with impersonate(create_user):
            other_ws = Workspace.objects.create(
                name="Other Workspace", slug="other-sched-ws", owner=create_user
            )
            foreign = Scheduler.objects.create(
                workspace=other_ws,
                slug="foreign",
                name="Foreign",
                prompt="Theirs.",
                source=SchedulerSource.MANIFEST,
            )

        response = api_key_client.patch(
            _definition_url(workspace.slug, foreign.id),
            {"prompt": "mine now"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        foreign.refresh_from_db()
        assert foreign.prompt == "Theirs."

    @pytest.mark.django_db
    @pytest.mark.parametrize("method", ["put", "post"])
    def test_only_patch_and_delete_are_exposed(
        self, api_key_client, workspace, custom_scheduler, method
    ):
        response = getattr(api_key_client, method)(
            _definition_url(workspace.slug, custom_scheduler.id), {}, format="json"
        )
        assert response.status_code == http_status.HTTP_405_METHOD_NOT_ALLOWED
        assert Scheduler.objects.filter(pk=custom_scheduler.pk).exists()


# --------------------------------------------------------------------------
# PDASHOSS01-318: create / delete a definition, install / uninstall it.


NEW_SCHEDULER = {
    "name": "Nightly Bug Scan",
    "slug": "nightly-bug-scan",
    "description": "Looks for regressions.",
    "prompt": "## Nightly\n\nScan for regressions and file issues.",
}


def _install_body(scheduler_ref, **extra):
    return {
        "scheduler": scheduler_ref,
        "dtstart": "2026-01-01T09:00:00Z",
        "rrule": "FREQ=DAILY",
        **extra,
    }


@pytest.mark.contract
class TestWorkspaceSchedulerCreate:
    @pytest.mark.django_db
    def test_admin_creates_definition(self, api_key_client, workspace, create_user):
        response = api_key_client.post(
            _ws_url(workspace.slug), NEW_SCHEDULER, format="json"
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        data = response.data
        for key, value in NEW_SCHEDULER.items():
            assert data[key] == value
        assert data["is_enabled"] is True
        assert data["is_builtin"] is False
        assert data["color"] == "#3b82f6"
        assert data["bindings"] == []
        created = Scheduler.objects.get(pk=data["id"])
        assert created.workspace_id == workspace.id
        # Audit: the caller is recorded on the row.
        assert created.created_by_id == create_user.id

        # The workspace list shows it.
        listed = api_key_client.get(_ws_url(workspace.slug))
        assert "nightly-bug-scan" in [r["slug"] for r in listed.data["results"]]

    @pytest.mark.django_db
    def test_disabled_and_color(self, api_key_client, workspace):
        response = api_key_client.post(
            _ws_url(workspace.slug),
            {**NEW_SCHEDULER, "is_enabled": False, "color": "#AABBCC"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert response.data["is_enabled"] is False
        assert response.data["color"] == "#aabbcc"

    @pytest.mark.django_db
    def test_duplicate_slug_is_a_field_error(
        self, api_key_client, workspace, scheduler
    ):
        response = api_key_client.post(
            _ws_url(workspace.slug),
            {**NEW_SCHEDULER, "slug": scheduler.slug},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "slug" in response.data
        assert Scheduler.objects.filter(slug=scheduler.slug).count() == 1

    @pytest.mark.django_db
    def test_slug_of_a_deleted_definition_is_reusable(
        self, api_key_client, workspace, scheduler
    ):
        scheduler.delete()

        response = api_key_client.post(
            _ws_url(workspace.slug),
            {**NEW_SCHEDULER, "slug": scheduler.slug},
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED

    @pytest.mark.django_db
    def test_builtin_slug_is_reserved(self, api_key_client, workspace):
        Scheduler.objects.filter(workspace=workspace, slug="security-audit").delete()

        response = api_key_client.post(
            _ws_url(workspace.slug),
            {**NEW_SCHEDULER, "slug": "security-audit"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {
            "slug": ["This slug is reserved for a built-in scheduler."]
        }

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "missing", ["name", "slug", "prompt"]
    )
    def test_required_fields(self, api_key_client, workspace, missing):
        body = {k: v for k, v in NEW_SCHEDULER.items() if k != missing}

        response = api_key_client.post(_ws_url(workspace.slug), body, format="json")

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert missing in response.data
        assert not Scheduler.objects.filter(name=NEW_SCHEDULER["name"]).exists()

    @pytest.mark.django_db
    def test_app_serializer_validation_applies(self, api_key_client, workspace):
        response = api_key_client.post(
            _ws_url(workspace.slug),
            {**NEW_SCHEDULER, "color": "blue"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "color" in response.data

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "extra", [{"source": "manifest"}, {"id": "x"}, {"bindings": []}]
    )
    def test_server_set_fields_rejected(self, api_key_client, workspace, extra):
        response = api_key_client.post(
            _ws_url(workspace.slug), {**NEW_SCHEDULER, **extra}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data["writable_fields"] == [
            "slug",
            "prompt",
            "name",
            "description",
            "color",
            "is_enabled",
        ]
        assert not Scheduler.objects.filter(slug="nightly-bug-scan").exists()

    @pytest.mark.django_db
    def test_non_object_body_rejected(self, api_key_client, workspace):
        response = api_key_client.post(
            _ws_url(workspace.slug), ["not", "an", "object"], format="json"
        )
        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "The request body must be a JSON object"}

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "client_fixture",
        [
            "project_admin_client",
            "member_client",
            "guest_client",
            "nonmember_client",
            "outside_client",
        ],
    )
    def test_non_workspace_admins_denied(self, request, workspace, client_fixture):
        client = request.getfixturevalue(client_fixture)

        response = client.post(_ws_url(workspace.slug), NEW_SCHEDULER, format="json")

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert not Scheduler.objects.filter(slug="nightly-bug-scan").exists()

    @pytest.mark.django_db
    def test_unauthenticated_denied(self, workspace):
        response = APIClient().post(
            _ws_url(workspace.slug), NEW_SCHEDULER, format="json"
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        assert not Scheduler.objects.filter(slug="nightly-bug-scan").exists()


@pytest.mark.contract
class TestWorkspaceSchedulerDefinitionEnable:
    @pytest.mark.django_db
    def test_admin_disables_and_recolors(
        self, api_key_client, workspace, custom_scheduler
    ):
        response = api_key_client.patch(
            _definition_url(workspace.slug, custom_scheduler.id),
            {"is_enabled": False, "color": "#112233"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["is_enabled"] is False
        assert response.data["previous"] == {"is_enabled": True, "color": "#3b82f6"}
        custom_scheduler.refresh_from_db()
        assert custom_scheduler.is_enabled is False


@pytest.mark.contract
class TestWorkspaceSchedulerDelete:
    @pytest.mark.django_db
    def test_admin_deletes_definition_and_every_install(
        self, api_key_client, workspace, sched_project, custom_scheduler, binding,
        create_user,
    ):
        with impersonate(create_user):
            other = Project.objects.create(
                name="Another Project", identifier="OTH", workspace=workspace
            )
            other_binding = SchedulerBinding.objects.create(
                scheduler=custom_scheduler,
                project=other,
                workspace=workspace,
                dtstart=timezone.now() - timedelta(days=1),
                rrule="FREQ=DAILY",
                tzid="UTC",
            )

        response = api_key_client.delete(
            _definition_url(workspace.slug, custom_scheduler.id)
        )

        assert response.status_code == http_status.HTTP_204_NO_CONTENT
        # Soft deletes: the rows stay, tombstoned.
        assert not Scheduler.objects.filter(pk=custom_scheduler.pk).exists()
        assert Scheduler.all_objects.get(pk=custom_scheduler.pk).deleted_at
        for b in (binding, other_binding):
            assert SchedulerBinding.all_objects.get(pk=b.pk).deleted_at is not None

        listed = api_key_client.get(_ws_url(workspace.slug))
        assert custom_scheduler.slug not in [r["slug"] for r in listed.data["results"]]
        project_list = api_key_client.get(_list_url(workspace.slug, sched_project.id))
        assert project_list.data["results"] == []

    @pytest.mark.django_db
    def test_builtin_rejected(self, api_key_client, workspace):
        builtin = Scheduler.objects.get(workspace=workspace, slug="security-audit")

        response = api_key_client.delete(_definition_url(workspace.slug, builtin.id))

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "builtin_scheduler"
        assert Scheduler.objects.filter(pk=builtin.pk).exists()

    @pytest.mark.django_db
    def test_stale_token_is_precondition_failed(
        self, api_key_client, workspace, custom_scheduler
    ):
        url = _definition_url(workspace.slug, custom_scheduler.id)

        response = api_key_client.delete(
            url, HTTP_IF_MATCH="2020-01-01T00:00:00+00:00"
        )

        assert response.status_code == http_status.HTTP_412_PRECONDITION_FAILED
        assert response.data["updated_at"] == custom_scheduler.updated_at.isoformat()
        assert Scheduler.objects.filter(pk=custom_scheduler.pk).exists()

        current = api_key_client.delete(
            url, HTTP_IF_MATCH=custom_scheduler.updated_at.isoformat()
        )
        assert current.status_code == http_status.HTTP_204_NO_CONTENT

    @pytest.mark.django_db
    def test_404_for_unknown_or_deleted(self, api_key_client, workspace, scheduler):
        scheduler.delete()
        response = api_key_client.delete(_definition_url(workspace.slug, scheduler.id))
        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "client_fixture",
        [
            "project_admin_client",
            "member_client",
            "guest_client",
            "nonmember_client",
            "outside_client",
        ],
    )
    def test_non_workspace_admins_denied(
        self, request, workspace, custom_scheduler, binding, client_fixture
    ):
        client = request.getfixturevalue(client_fixture)

        response = client.delete(_definition_url(workspace.slug, custom_scheduler.id))

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert Scheduler.objects.filter(pk=custom_scheduler.pk).exists()
        assert SchedulerBinding.objects.filter(pk=binding.pk).exists()

    @pytest.mark.django_db
    def test_unauthenticated_denied(self, workspace, custom_scheduler):
        response = APIClient().delete(
            _definition_url(workspace.slug, custom_scheduler.id)
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        assert Scheduler.objects.filter(pk=custom_scheduler.pk).exists()


@pytest.mark.contract
class TestProjectSchedulerInstall:
    @pytest.mark.django_db
    def test_admin_installs_by_id(
        self, api_key_client, workspace, sched_project, scheduler, create_user
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(str(scheduler.id), extra_context="Only the API."),
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        data = response.data
        assert str(data["scheduler"]) == str(scheduler.id)
        assert str(data["project"]) == str(sched_project.id)
        assert data["scheduler_slug"] == scheduler.slug
        assert data["rrule"] == "FREQ=DAILY"
        assert data["extra_context"] == "Only the API."
        assert data["enabled"] is True
        # next_run_at is set on install, as the web app does.
        assert data["next_run_at"] is not None
        binding = SchedulerBinding.objects.get(pk=data["id"])
        assert binding.next_run_at > timezone.now()
        assert binding.actor_id == create_user.id

        listed = api_key_client.get(_list_url(workspace.slug, sched_project.id))
        rows = listed.data["results"]
        assert [r["slug"] for r in rows] == [scheduler.slug]
        assert rows[0]["bindings"][0]["next_run_at"] is not None

    @pytest.mark.django_db
    def test_admin_installs_builtin_by_slug(
        self, api_key_client, workspace, sched_project
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body("security-audit", tzid="America/New_York", enabled=False),
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert response.data["scheduler_slug"] == "security-audit"
        assert response.data["tzid"] == "America/New_York"
        assert response.data["enabled"] is False

    @pytest.mark.django_db
    def test_pod_from_the_project(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        pod = Pod.default_for_project_id(sched_project.id)

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug, pod=str(pod.id)),
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert str(response.data["pod"]) == str(pod.id)

    @pytest.mark.django_db
    def test_reinstall_after_uninstall(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        api_key_client.delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id)
        )

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED

    @pytest.mark.django_db
    def test_already_installed(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "already_installed"
        assert SchedulerBinding.objects.filter(scheduler=scheduler).count() == 1

    @pytest.mark.django_db
    def test_disabled_definition(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        scheduler.is_enabled = False
        scheduler.save(update_fields=["is_enabled"])

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["code"] == "scheduler_disabled"
        assert not SchedulerBinding.objects.filter(scheduler=scheduler).exists()

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "ref", ["no-such-scheduler", "00000000-0000-0000-0000-000000000000"]
    )
    def test_unknown_scheduler(self, api_key_client, workspace, sched_project, ref):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(ref),
            format="json",
        )
        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        assert response.data == {"error": "Scheduler not found"}

    @pytest.mark.django_db
    def test_another_workspaces_scheduler(
        self, api_key_client, workspace, sched_project, create_user
    ):
        with impersonate(create_user):
            other_ws = Workspace.objects.create(
                name="Other Workspace", slug="other-sched-ws", owner=create_user
            )
            foreign = Scheduler.objects.create(
                workspace=other_ws, slug="foreign", name="Foreign", prompt="Theirs."
            )

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(str(foreign.id)),
            format="json",
        )

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
        assert not SchedulerBinding.objects.filter(scheduler=foreign).exists()

    @pytest.mark.django_db
    @pytest.mark.parametrize("ref", [None, "", "  ", 5])
    def test_scheduler_required(
        self, api_key_client, workspace, sched_project, ref
    ):
        body = _install_body(ref)
        if ref is None:
            del body["scheduler"]

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id), body, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {
            "scheduler": ["The scheduler's id or slug is required."]
        }

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "extra, field",
        [
            ({"rrule": "FREQ=NEVER"}, "rrule"),
            ({"tzid": "Mars/Olympus"}, "tzid"),
            ({"dtstart": "not a date"}, "dtstart"),
        ],
    )
    def test_app_serializer_validation_applies(
        self, api_key_client, workspace, sched_project, scheduler, extra, field
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug, **extra),
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert field in response.data
        assert not SchedulerBinding.objects.filter(scheduler=scheduler).exists()

    @pytest.mark.django_db
    def test_dtstart_required(
        self, api_key_client, workspace, sched_project, scheduler
    ):
        body = _install_body(scheduler.slug)
        del body["dtstart"]

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id), body, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "dtstart" in response.data

    @pytest.mark.django_db
    def test_pod_from_another_project(
        self, api_key_client, workspace, sched_project, scheduler, create_user
    ):
        with impersonate(create_user):
            other = Project.objects.create(
                name="Another Project", identifier="OTH", workspace=workspace
            )
        foreign_pod = Pod.default_for_project_id(other.id)

        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug, pod=str(foreign_pod.id)),
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert "pod" in response.data

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "extra",
        [
            {"project": "00000000-0000-0000-0000-000000000000"},
            {"next_run_at": "2030-01-01T00:00:00Z"},
            {"prompt": "belongs on the definition"},
        ],
    )
    def test_non_install_fields_rejected(
        self, api_key_client, workspace, sched_project, scheduler, extra
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug, **extra),
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data["writable_fields"][0] == "scheduler"
        assert not SchedulerBinding.objects.filter(scheduler=scheduler).exists()

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "client_fixture",
        ["member_client", "guest_client", "nonmember_client", "outside_client"],
    )
    def test_non_admins_denied(
        self, request, workspace, sched_project, scheduler, client_fixture
    ):
        client = request.getfixturevalue(client_fixture)

        response = client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert not SchedulerBinding.objects.filter(scheduler=scheduler).exists()

    @pytest.mark.django_db
    def test_project_admin_who_is_workspace_member_may_install(
        self, project_admin_client, workspace, sched_project, scheduler
    ):
        response = project_admin_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )
        assert response.status_code == http_status.HTTP_201_CREATED

    @pytest.mark.django_db
    def test_unauthenticated_denied(self, workspace, sched_project, scheduler):
        response = APIClient().post(
            _list_url(workspace.slug, sched_project.id),
            _install_body(scheduler.slug),
            format="json",
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        assert not SchedulerBinding.objects.filter(scheduler=scheduler).exists()


@pytest.mark.contract
class TestProjectSchedulerUninstall:
    @pytest.mark.django_db
    def test_admin_uninstalls(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        response = api_key_client.delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id)
        )

        assert response.status_code == http_status.HTTP_204_NO_CONTENT
        assert SchedulerBinding.all_objects.get(pk=binding.pk).deleted_at is not None
        # The definition stays.
        assert Scheduler.objects.filter(pk=scheduler.pk).exists()
        listed = api_key_client.get(_list_url(workspace.slug, sched_project.id))
        assert listed.data["results"] == []

        again = api_key_client.delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert again.status_code == http_status.HTTP_404_NOT_FOUND

    @pytest.mark.django_db
    def test_builtin_can_be_uninstalled(
        self, api_key_client, workspace, sched_project
    ):
        installed = api_key_client.post(
            _list_url(workspace.slug, sched_project.id),
            _install_body("security-audit"),
            format="json",
        )
        builtin_id = installed.data["scheduler"]

        response = api_key_client.delete(
            _binding_url(workspace.slug, sched_project.id, builtin_id)
        )

        assert response.status_code == http_status.HTTP_204_NO_CONTENT
        assert Scheduler.objects.filter(pk=builtin_id).exists()

    @pytest.mark.django_db
    def test_stale_token_is_precondition_failed(
        self, api_key_client, workspace, sched_project, scheduler, binding
    ):
        response = api_key_client.delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id),
            HTTP_IF_MATCH="2020-01-01T00:00:00+00:00",
        )

        assert response.status_code == http_status.HTTP_412_PRECONDITION_FAILED
        assert SchedulerBinding.objects.filter(pk=binding.pk).exists()

    @pytest.mark.django_db
    @pytest.mark.parametrize(
        "client_fixture",
        ["member_client", "guest_client", "nonmember_client", "outside_client"],
    )
    def test_non_admins_denied(
        self, request, workspace, sched_project, scheduler, binding, client_fixture
    ):
        client = request.getfixturevalue(client_fixture)

        response = client.delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id)
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert SchedulerBinding.objects.filter(pk=binding.pk).exists()

    @pytest.mark.django_db
    def test_unauthenticated_denied(self, workspace, sched_project, scheduler, binding):
        response = APIClient().delete(
            _binding_url(workspace.slug, sched_project.id, scheduler.id)
        )
        assert response.status_code == http_status.HTTP_401_UNAUTHORIZED
        assert SchedulerBinding.objects.filter(pk=binding.pk).exists()
