# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``GET /api/runners/projects/`` carries ``Project.context`` next to ``description``."""

import pytest
from rest_framework import status

from pi_dash.db.models import Project


@pytest.mark.contract
@pytest.mark.django_db
def test_project_list_returns_context(session_client, workspace, project):
    Project.objects.filter(pk=project.id).update(description="Summary", context="Never push to main.")

    response = session_client.get("/api/runners/projects/", {"workspace": str(workspace.id)})

    assert response.status_code == status.HTTP_200_OK, response.data
    row = next(r for r in response.data if r["id"] == str(project.id))
    assert row["context"] == "Never push to main."
    assert row["description"] == "Summary"
