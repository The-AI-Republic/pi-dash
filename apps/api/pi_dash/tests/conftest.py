# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from rest_framework.test import APIClient
from pytest_django.fixtures import django_db_setup

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.db.models.api import APIToken


@pytest.fixture(scope="session")
def django_db_setup(django_db_setup):  # noqa: F811
    """Set up the Django database for the test session"""
    pass


@pytest.fixture
def api_client():
    """Return an unauthenticated API client"""
    return APIClient()


@pytest.fixture
def user_data():
    """Return standard user data for tests"""
    return {
        "email": "test@example.com",
        "password": "test-password",
        "first_name": "Test",
        "last_name": "User",
    }


@pytest.fixture
def create_user(db, user_data):
    """Create and return a user instance"""
    user = User.objects.create(
        email=user_data["email"],
        first_name=user_data["first_name"],
        last_name=user_data["last_name"],
    )
    user.set_password(user_data["password"])
    user.save()
    return user


@pytest.fixture
def api_token(db, create_user):
    """Create and return an API token for testing the external API"""
    token = APIToken.objects.create(
        user=create_user,
        label="Test API Token",
        token="test-api-token-12345",
    )
    return token


@pytest.fixture
def api_key_client(api_client, api_token):
    """Return an API key authenticated client for external API testing"""
    api_client.credentials(HTTP_X_API_KEY=api_token.token)
    return api_client


@pytest.fixture
def session_client(api_client, create_user):
    """Return a session authenticated API client for app API testing, which is what pi-dash.app uses"""
    api_client.force_authenticate(user=create_user)
    return api_client


@pytest.fixture
def create_bot_user(db):
    """Create and return a bot user instance"""
    from uuid import uuid4

    unique_id = uuid4().hex[:8]
    user = User.objects.create(
        email=f"bot-{unique_id}@example.com",
        username=f"bot_user_{unique_id}",
        first_name="Bot",
        last_name="User",
        is_bot=True,
    )
    user.set_password("bot@123")
    user.save()
    return user


@pytest.fixture
def api_token_data():
    """Return sample API token data for testing"""
    from django.utils import timezone
    from datetime import timedelta

    return {
        "label": "Test API Token",
        "description": "Test description for API token",
        "expired_at": (timezone.now() + timedelta(days=30)).isoformat(),
    }


@pytest.fixture
def create_api_token_for_user(db, create_user):
    """Create and return an API token for a specific user"""
    return APIToken.objects.create(
        label="Test Token",
        description="Test token description",
        user=create_user,
        user_type=0,
    )


@pytest.fixture
def pi_dash_server(live_server):
    """
    Renamed version of live_server fixture to avoid name clashes.
    Returns a live Django server for testing HTTP requests.
    """
    return live_server


@pytest.fixture
def workspace(create_user):
    """
    Create a new workspace, a default project, and return the
    corresponding Workspace model instance.

    Pods are project-scoped post-refactor (see
    ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/``);
    runner / pod / agent_run tests that previously relied on a workspace-default
    pod now need at least one Project so the workspace has a default pod via
    the project's auto-creation. To keep every existing test working without
    explicit ``project=`` plumbing, the workspace fixture creates a single
    Project named "TEST"; tests that need a *second* project should depend on
    the dedicated ``project`` fixture (and / or create more directly).
    """
    from pi_dash.db.models.project import Project

    created_workspace = Workspace.objects.create(
        name="Test Workspace",
        owner=create_user,
        slug="test-workspace",
    )

    WorkspaceMember.objects.create(workspace=created_workspace, member=create_user, role=20)

    # Create a default Project so the post_save(Project) signal auto-creates
    # the project's default pod, and any test that creates a Runner with
    # ``workspace=workspace`` and no explicit pod can fall through
    # ``Runner.save()``'s single-project auto-resolution.
    Project.objects.create(
        name="Test Project",
        identifier="TEST",
        workspace=created_workspace,
        created_by=create_user,
    )
    return created_workspace


@pytest.fixture
def project(workspace, create_user):
    """The default Project created by the ``workspace`` fixture.

    Resolved by identifier rather than re-created so multiple fixtures that
    depend on ``workspace`` see the same Project instance.
    """
    from pi_dash.db.models.project import Project

    return Project.objects.get(workspace=workspace, identifier="TEST")
