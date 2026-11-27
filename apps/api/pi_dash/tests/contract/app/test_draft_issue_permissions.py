# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the workspace draft creator bypass (PDASHOSS01-320).

DRAFT-025: update, single fetch and delete of a draft are allowed for a
workspace admin or the draft's creator, at any workspace role including
guest. The creator bypass must look the draft up in the draft table; it
used to read work-item rows for update/retrieve, so every non-admin owner
was refused. A non-creator member/guest stays refused.
"""

from unittest.mock import patch

import pytest
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.db.models import DraftIssue, User, WorkspaceMember


def _drafts_url(slug):
    return f"/api/workspaces/{slug}/draft-issues/"


def _draft_url(slug, draft_id):
    return f"/api/workspaces/{slug}/draft-issues/{draft_id}/"


def _client_for(user):
    client = APIClient()
    client.force_authenticate(user=user)
    return client


@pytest.fixture
def member_user(db):
    return User.objects.create_user(email="member@example.com", username="member")


@pytest.fixture
def guest_user(db):
    return User.objects.create_user(email="guest@example.com", username="guest")


@pytest.fixture
def memberships(workspace, member_user, guest_user):
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    WorkspaceMember.objects.create(workspace=workspace, member=guest_user, role=5)


def _create_draft(user, slug, name):
    response = _client_for(user).post(_drafts_url(slug), {"name": name}, format="json")
    assert response.status_code == status.HTTP_201_CREATED, response.data
    return response.data["id"]


@pytest.mark.contract
class TestDraftCreatorBypass:
    @pytest.mark.django_db
    def test_guest_updates_and_fetches_own_draft(self, workspace, guest_user, memberships):
        draft_id = _create_draft(guest_user, workspace.slug, "guest draft")
        client = _client_for(guest_user)

        response = client.patch(_draft_url(workspace.slug, draft_id), {"name": "renamed"}, format="json")
        assert response.status_code == status.HTTP_204_NO_CONTENT

        response = client.get(_draft_url(workspace.slug, draft_id))
        assert response.status_code == status.HTTP_200_OK
        assert response.data["name"] == "renamed"

    @pytest.mark.django_db
    def test_member_fetches_and_updates_own_draft(self, workspace, member_user, memberships):
        draft_id = _create_draft(member_user, workspace.slug, "member draft")
        client = _client_for(member_user)

        response = client.get(_draft_url(workspace.slug, draft_id))
        assert response.status_code == status.HTTP_200_OK
        assert response.data["name"] == "member draft"

        response = client.patch(_draft_url(workspace.slug, draft_id), {"name": "renamed"}, format="json")
        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert DraftIssue.objects.get(pk=draft_id).name == "renamed"

    @pytest.mark.django_db
    def test_guest_deletes_own_draft(self, workspace, guest_user, memberships):
        draft_id = _create_draft(guest_user, workspace.slug, "guest draft")

        with patch("pi_dash.db.mixins.soft_delete_related_objects.delay"):
            response = _client_for(guest_user).delete(_draft_url(workspace.slug, draft_id))
        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert not DraftIssue.objects.filter(pk=draft_id).exists()

    @pytest.mark.django_db
    def test_guest_refused_on_another_users_draft(self, workspace, create_user, guest_user, memberships):
        draft_id = _create_draft(create_user, workspace.slug, "admin draft")
        client = _client_for(guest_user)

        assert client.get(_draft_url(workspace.slug, draft_id)).status_code == status.HTTP_403_FORBIDDEN
        response = client.patch(_draft_url(workspace.slug, draft_id), {"name": "hijack"}, format="json")
        assert response.status_code == status.HTTP_403_FORBIDDEN
        assert client.delete(_draft_url(workspace.slug, draft_id)).status_code == status.HTTP_403_FORBIDDEN
        assert DraftIssue.objects.get(pk=draft_id).name == "admin draft"

    @pytest.mark.django_db
    def test_member_refused_on_another_users_draft(self, workspace, create_user, member_user, memberships):
        draft_id = _create_draft(create_user, workspace.slug, "admin draft")
        client = _client_for(member_user)

        assert client.get(_draft_url(workspace.slug, draft_id)).status_code == status.HTTP_403_FORBIDDEN
        # Members pass the PATCH role check, but the own-drafts lookup misses.
        response = client.patch(_draft_url(workspace.slug, draft_id), {"name": "hijack"}, format="json")
        assert response.status_code == status.HTTP_404_NOT_FOUND
        assert client.delete(_draft_url(workspace.slug, draft_id)).status_code == status.HTTP_403_FORBIDDEN
        assert DraftIssue.objects.get(pk=draft_id).name == "admin draft"

    @pytest.mark.django_db
    def test_guest_convert_still_refused(self, workspace, guest_user, memberships):
        draft_id = _create_draft(guest_user, workspace.slug, "guest draft")

        response = _client_for(guest_user).post(f"/api/workspaces/{workspace.slug}/draft-to-issue/{draft_id}/")
        assert response.status_code == status.HTTP_403_FORBIDDEN
