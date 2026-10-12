# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``Project.context`` round-trips through the public ``/api/v1/`` project endpoints."""

import pytest
from rest_framework import status

from pi_dash.db.models import Project, ProjectMember


def _url(workspace, pk=None):
    base = f"/api/v1/workspaces/{workspace.slug}/projects/"
    return f"{base}{pk}/" if pk else base


@pytest.mark.contract
@pytest.mark.django_db
def test_create_project_accepts_context(api_key_client, workspace):
    response = api_key_client.post(
        _url(workspace),
        {"name": "Agent Project", "identifier": "AGP", "description": "Summary", "context": "Never push to main."},
        format="json",
    )

    assert response.status_code == status.HTTP_201_CREATED, response.data
    assert response.data["context"] == "Never push to main."
    assert response.data["description"] == "Summary"
    assert Project.objects.get(pk=response.data["id"]).context == "Never push to main."


@pytest.mark.contract
@pytest.mark.django_db
def test_update_and_retrieve_project_context(api_key_client, workspace, project, create_user):
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    Project.objects.filter(pk=project.id).update(description="Summary")
    url = _url(workspace, project.id)

    response = api_key_client.patch(url, {"context": "Run the linter before pushing."}, format="json")

    assert response.status_code == status.HTTP_200_OK, response.data
    assert response.data["context"] == "Run the linter before pushing."

    response = api_key_client.get(url)
    assert response.status_code == status.HTTP_200_OK
    assert response.data["context"] == "Run the linter before pushing."
    assert response.data["description"] == "Summary"
