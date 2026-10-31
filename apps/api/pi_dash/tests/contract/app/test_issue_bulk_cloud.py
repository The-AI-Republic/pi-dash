# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the bulk issue endpoints (NEWFRONT-984, ISS-112/115).

Bulk update applies one properties object across a selection: scalars
replace, array-valued properties union onto the stored rows, and the
whole batch validates before anything is written. Bulk subscribe
subscribes the caller idempotently. Both ride the OSS stack API
ungated, per the NEWFRONT-672 precedent — the cloud gate lives in the
web_new edition module, not here.
"""

from datetime import timedelta
from unittest.mock import patch
from uuid import uuid4

import pytest
from django.utils import timezone
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import (
    Cycle,
    CycleIssue,
    Estimate,
    EstimatePoint,
    Issue,
    IssueAssignee,
    IssueLabel,
    IssueSubscriber,
    Label,
    Module,
    ModuleIssue,
    ProjectMember,
    State,
    User,
    WorkspaceMember,
)


def _operation_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/bulk-operation-issues/"


def _subscribe_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/bulk-subscribe-issues/"


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="member@example.com", username="member")


@pytest.fixture
def other_member_user(db):
    return User.objects.create_user(email="other@example.com", username="other")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="guest@example.com", username="guest")


@pytest.fixture
def outsider_user(db):
    return User.objects.create_user(email="outsider@example.com", username="outsider")


@pytest.fixture
def memberships(workspace, project, create_user, member_user, other_member_user, guest_user):
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    ProjectMember.objects.create(project=project, member=member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=other_member_user, role=15)
    ProjectMember.objects.create(project=project, member=other_member_user, role=15, is_active=True)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)
    ProjectMember.objects.create(project=project, member=guest_user, role=5, is_active=True)


@pytest.fixture
def todo_state(workspace, project, create_user):
    return State.objects.create(
        name="Todo",
        project=project,
        workspace=workspace,
        group="unstarted",
        default=True,
        created_by=create_user,
    )


@pytest.fixture
def done_state(workspace, project, create_user):
    return State.objects.create(
        name="Done",
        project=project,
        workspace=workspace,
        group="completed",
        created_by=create_user,
    )


def _make_issue(name, project, workspace, state, creator):
    # BaseModel.save() overwrites created_by from the request user (None in
    # tests), so save explicitly to keep the creator the scoping relies on.
    issue = Issue(
        name=name,
        description_html="<p>body</p>",
        description_stripped="body",
        project=project,
        workspace=workspace,
        state=state,
        created_by=creator,
    )
    issue.save(disable_auto_set_user=True)
    return issue


@pytest.fixture
def issue_pair(workspace, project, create_user, todo_state):
    return [
        _make_issue("bulk one", project, workspace, todo_state, create_user),
        _make_issue("bulk two", project, workspace, todo_state, create_user),
    ]


@pytest.fixture
def label_pair(workspace, project, create_user):
    return [
        Label.objects.create(
            name=f"bulk label {index}",
            project=project,
            workspace=workspace,
            created_by=create_user,
        )
        for index in (1, 2)
    ]


@pytest.fixture
def module_pair(workspace, project, create_user):
    return [
        Module.objects.create(
            name=f"bulk module {index}",
            project=project,
            workspace=workspace,
            created_by=create_user,
        )
        for index in (1, 2)
    ]


@pytest.fixture
def cycle_pair(workspace, project, create_user):
    return [
        Cycle.objects.create(
            name=f"bulk cycle {index}",
            project=project,
            workspace=workspace,
            owned_by=create_user,
            start_date=timezone.now() + timedelta(days=1),
            end_date=timezone.now() + timedelta(days=7),
        )
        for index in (1, 2)
    ]


@pytest.fixture
def estimate_point(workspace, project, create_user):
    estimate = Estimate.objects.create(
        name="bulk estimate",
        project=project,
        workspace=workspace,
        created_by=create_user,
    )
    return EstimatePoint.objects.create(
        estimate=estimate,
        key=3,
        value="3",
        project=project,
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture(autouse=True)
def no_celery():
    with (
        patch("pi_dash.app.views.issue.bulk_operations.issue_activity") as activity,
        patch("pi_dash.db.mixins.soft_delete_related_objects.delay"),
    ):
        yield activity


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


def _activity_types(no_celery):
    return [call.kwargs["type"] for call in no_celery.delay.call_args_list]


@pytest.mark.contract
class TestBulkOperationUpdate:
    @pytest.mark.django_db
    def test_member_updates_scalars_across_selection(
        self, workspace, project, member_user, memberships, issue_pair, done_state, estimate_point, no_celery
    ):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {
                "issue_ids": [str(issue.id) for issue in issue_pair],
                "properties": {
                    "state_id": str(done_state.id),
                    "priority": "high",
                    "start_date": "2026-11-10",
                    "target_date": "2026-11-17",
                    "estimate_point": str(estimate_point.id),
                },
            },
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["updated"] == 2
        assert "2" in response.data["message"]
        for issue in issue_pair:
            issue.refresh_from_db()
            assert issue.state_id == done_state.id
            assert issue.priority == "high"
            assert str(issue.start_date) == "2026-11-10"
            assert str(issue.target_date) == "2026-11-17"
            assert issue.estimate_point_id == estimate_point.id
        assert _activity_types(no_celery) == ["issue.activity.updated", "issue.activity.updated"]
        requested = no_celery.delay.call_args_list[0].kwargs["requested_data"]
        assert '"priority": "high"' in requested

    @pytest.mark.django_db
    def test_arrays_union_onto_existing_rows(
        self,
        workspace,
        project,
        member_user,
        other_member_user,
        memberships,
        issue_pair,
        label_pair,
        module_pair,
        no_celery,
    ):
        first = issue_pair[0]
        IssueLabel.objects.create(
            issue=first, label=label_pair[0], project=project, workspace=workspace
        )
        IssueAssignee.objects.create(
            issue=first, assignee=member_user, project=project, workspace=workspace
        )
        ModuleIssue.objects.create(
            issue=first, module=module_pair[0], project=project, workspace=workspace
        )

        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {
                "issue_ids": [str(first.id)],
                "properties": {
                    "label_ids": [str(label.id) for label in label_pair],
                    "assignee_ids": [str(member_user.id), str(other_member_user.id)],
                    "module_ids": [str(module.id) for module in module_pair],
                },
            },
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert IssueLabel.objects.filter(issue=first, deleted_at__isnull=True).count() == 2
        assert IssueAssignee.objects.filter(issue=first, deleted_at__isnull=True).count() == 2
        assert ModuleIssue.objects.filter(issue=first, deleted_at__isnull=True).count() == 2
        assert "module.activity.created" in _activity_types(no_celery)
        assert "issue.activity.updated" in _activity_types(no_celery)

    @pytest.mark.django_db
    def test_cycle_moves_between_cycles(
        self, workspace, project, member_user, memberships, issue_pair, cycle_pair, no_celery
    ):
        first = issue_pair[0]
        CycleIssue.objects.create(
            issue=first, cycle=cycle_pair[0], project=project, workspace=workspace
        )

        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(first.id)], "properties": {"cycle_id": str(cycle_pair[1].id)}},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        rows = list(CycleIssue.objects.filter(issue=first, deleted_at__isnull=True))
        assert len(rows) == 1
        assert rows[0].cycle_id == cycle_pair[1].id
        assert "cycle.activity.created" in _activity_types(no_celery)

    @pytest.mark.django_db
    def test_null_cycle_clears_membership(
        self, workspace, project, member_user, memberships, issue_pair, cycle_pair, no_celery
    ):
        first = issue_pair[0]
        CycleIssue.objects.create(
            issue=first, cycle=cycle_pair[0], project=project, workspace=workspace
        )

        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(first.id)], "properties": {"cycle_id": None}},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert CycleIssue.objects.filter(issue=first, deleted_at__isnull=True).count() == 0
        assert "cycle.activity.deleted" in _activity_types(no_celery)

    @pytest.mark.django_db
    def test_merged_date_violation_fails_batch_untouched(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        first, second = issue_pair
        first.start_date = "2026-11-10"
        first.save()

        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {
                "issue_ids": [str(first.id), str(second.id)],
                "properties": {"target_date": "2026-11-01"},
            },
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        first.refresh_from_db()
        second.refresh_from_db()
        assert first.target_date is None
        assert second.target_date is None

    @pytest.mark.django_db
    def test_unknown_property_is_rejected(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)], "properties": {"name": "renamed"}},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_empty_properties_are_rejected(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)], "properties": {}},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_empty_selection_is_rejected(self, workspace, project, member_user, memberships):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [], "properties": {"priority": "high"}},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_unknown_reference_fails_batch_untouched(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {
                "issue_ids": [str(issue_pair[0].id)],
                "properties": {"state_id": str(uuid4()), "priority": "urgent"},
            },
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
        issue_pair[0].refresh_from_db()
        assert issue_pair[0].priority == "none"

    @pytest.mark.django_db
    def test_unknown_ids_are_silently_skipped(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {
                "issue_ids": [str(issue_pair[0].id), str(uuid4())],
                "properties": {"priority": "low"},
            },
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["updated"] == 1
        issue_pair[0].refresh_from_db()
        assert issue_pair[0].priority == "low"

    @pytest.mark.django_db
    def test_completed_cycle_is_refused(
        self, workspace, project, create_user, member_user, memberships, issue_pair
    ):
        done = Cycle.objects.create(
            name="done cycle",
            project=project,
            workspace=workspace,
            owned_by=create_user,
            start_date=timezone.now() - timedelta(days=7),
            end_date=timezone.now() - timedelta(days=1),
        )

        response = _client_for(member_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)], "properties": {"cycle_id": str(done.id)}},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST

    @pytest.mark.django_db
    def test_guest_is_refused(self, workspace, project, guest_user, memberships, issue_pair):
        response = _client_for(guest_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)], "properties": {"priority": "high"}},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_outsider_is_refused(self, workspace, project, outsider_user, memberships, issue_pair):
        response = _client_for(outsider_user).post(
            _operation_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)], "properties": {"priority": "high"}},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN


@pytest.mark.contract
class TestBulkSubscribe:
    @pytest.mark.django_db
    def test_member_subscribes_to_selection(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        response = _client_for(member_user).post(
            _subscribe_url(workspace.slug, project.id),
            {"issue_ids": [str(issue.id) for issue in issue_pair]},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["subscribed"] == 2
        for issue in issue_pair:
            assert IssueSubscriber.objects.filter(
                issue=issue, subscriber=member_user, deleted_at__isnull=True
            ).exists()

    @pytest.mark.django_db
    def test_resubscribe_is_idempotent(
        self, workspace, project, member_user, memberships, issue_pair
    ):
        client = _client_for(member_user)
        url = _subscribe_url(workspace.slug, project.id)
        payload = {"issue_ids": [str(issue.id) for issue in issue_pair]}

        assert client.post(url, payload, format="json").status_code == status.HTTP_200_OK
        response = client.post(url, payload, format="json")

        assert response.status_code == status.HTTP_200_OK
        assert response.data["subscribed"] == 0
        assert (
            IssueSubscriber.objects.filter(
                issue_id__in=[issue.id for issue in issue_pair],
                subscriber=member_user,
                deleted_at__isnull=True,
            ).count()
            == 2
        )

    @pytest.mark.django_db
    def test_guest_can_subscribe(self, workspace, project, guest_user, memberships, issue_pair):
        response = _client_for(guest_user).post(
            _subscribe_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)]},
            format="json",
        )

        assert response.status_code == status.HTTP_200_OK
        assert response.data["subscribed"] == 1

    @pytest.mark.django_db
    def test_outsider_is_refused(self, workspace, project, outsider_user, memberships, issue_pair):
        response = _client_for(outsider_user).post(
            _subscribe_url(workspace.slug, project.id),
            {"issue_ids": [str(issue_pair[0].id)]},
            format="json",
        )

        assert response.status_code == status.HTTP_403_FORBIDDEN

    @pytest.mark.django_db
    def test_empty_selection_is_rejected(self, workspace, project, member_user, memberships):
        response = _client_for(member_user).post(
            _subscribe_url(workspace.slug, project.id),
            {"issue_ids": []},
            format="json",
        )

        assert response.status_code == status.HTTP_400_BAD_REQUEST
