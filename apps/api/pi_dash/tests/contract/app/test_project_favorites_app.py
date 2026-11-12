# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from rest_framework import status

from pi_dash.db.models import UserFavorite


def favorites_url(slug, project_id=None):
    base = f"/api/workspaces/{slug}/user-favorite-projects/"
    return f"{base}{project_id}/" if project_id else base


@pytest.mark.contract
class TestProjectFavorites:
    @pytest.mark.django_db
    def test_list_empty(self, session_client, workspace):
        response = session_client.get(favorites_url(workspace.slug))

        assert response.status_code == status.HTTP_200_OK
        assert response.data == []

    @pytest.mark.django_db
    def test_add_list_remove(self, session_client, workspace, project):
        response = session_client.post(favorites_url(workspace.slug), {"project": str(project.id)}, format="json")
        assert response.status_code == status.HTTP_204_NO_CONTENT

        response = session_client.get(favorites_url(workspace.slug))
        assert response.status_code == status.HTTP_200_OK
        assert len(response.data) == 1
        favorite = response.data[0]
        assert list(favorite.keys()) == [
            "id",
            "entity_type",
            "entity_identifier",
            "entity_data",
            "name",
            "is_folder",
            "sequence",
            "parent",
            "workspace_id",
            "project_id",
        ]
        assert favorite["entity_type"] == "project"
        assert str(favorite["entity_identifier"]) == str(project.id)
        assert str(favorite["project_id"]) == str(project.id)
        assert str(favorite["workspace_id"]) == str(workspace.id)
        assert favorite["entity_data"] == {
            "id": str(project.id),
            "name": project.name,
            "logo_props": project.logo_props,
        }

        response = session_client.delete(favorites_url(workspace.slug, project.id))
        assert response.status_code == status.HTTP_204_NO_CONTENT
        assert not UserFavorite.objects.filter(entity_identifier=project.id, entity_type="project").exists()

        response = session_client.get(favorites_url(workspace.slug))
        assert response.status_code == status.HTTP_200_OK
        assert response.data == []

    @pytest.mark.django_db
    def test_list_only_own_favorites(self, session_client, workspace, project, create_user):
        from pi_dash.db.models import User

        other = User.objects.create(email="other-fav@pi-dash.so", username="other-fav")
        UserFavorite.objects.create(
            user=other,
            workspace=workspace,
            entity_type="project",
            entity_identifier=project.id,
            project=project,
        )

        response = session_client.get(favorites_url(workspace.slug))
        assert response.status_code == status.HTTP_200_OK
        assert response.data == []
