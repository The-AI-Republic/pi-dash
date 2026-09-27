# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the ``/api/v1/`` project page read endpoints.

These are the only way an agent (``pidash page …``, the MCP page tool) can
read a project wiki, so the contract under test is the *caller's* view:
who can see which page, what a list row carries, and what a detail payload
carries.
"""

import pytest
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Page, Project, ProjectMember, ProjectPage, State, User


@pytest.fixture
def page_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Page Test Project",
        identifier="PG",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def other_user(db):
    return User.objects.create(
        email="someone-else@example.com",
        username="someone-else",
        first_name="Other",
    )


def _make_page(project, owner, *, name="A page", html="<p>body</p>", access=0, archived_at=None, parent=None):
    page = Page.objects.create(
        name=name,
        description_html=html,
        owned_by=owner,
        access=access,
        archived_at=archived_at,
        parent=parent,
        workspace=project.workspace,
    )
    ProjectPage.objects.create(workspace=project.workspace, project=project, page=page)
    return page


def _list_url(slug, project_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/"


def _detail_url(slug, project_id, page_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/"


@pytest.mark.contract
@pytest.mark.django_db
class TestPageList:
    def test_member_lists_project_pages(self, api_key_client, workspace, page_project, create_user):
        _make_page(page_project, create_user, name="Conventions")

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert [row["name"] for row in response.data["results"]] == ["Conventions"]

    def test_list_rows_carry_metadata_but_no_body(self, api_key_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        _make_page(page_project, create_user, name="Child", parent=parent)

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        child = next(row for row in response.data["results"] if row["name"] == "Child")
        assert set(child) == {
            "id",
            "name",
            "parent",
            "owned_by",
            "access",
            "is_locked",
            "archived_at",
            "created_at",
            "updated_at",
        }
        assert str(child["parent"]) == str(parent.id)

    def test_pagination_envelope_matches_other_v1_list_endpoints(
        self, api_key_client, workspace, page_project, create_user
    ):
        """The agent parses one envelope shape across every v1 list route, so
        compare against a sibling endpoint rather than a hardcoded key list."""
        _make_page(page_project, create_user)
        State.objects.create(
            name="Todo",
            project=page_project,
            workspace=workspace,
            group="unstarted",
            created_by=create_user,
        )

        pages = api_key_client.get(_list_url(workspace.slug, page_project.id))
        states = api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/projects/{page_project.id}/states/")

        assert set(pages.data) == set(states.data)

    def test_per_page_and_cursor_walk_pages(self, api_key_client, workspace, page_project, create_user):
        for index in range(3):
            _make_page(page_project, create_user, name=f"Page {index}")

        first = api_key_client.get(_list_url(workspace.slug, page_project.id), {"per_page": 2})

        assert first.data["count"] == 2
        assert first.data["total_count"] == 3

        second = api_key_client.get(
            _list_url(workspace.slug, page_project.id),
            {"per_page": 2, "cursor": first.data["next_cursor"]},
        )

        assert second.data["count"] == 1
        first_ids = {row["id"] for row in first.data["results"]}
        assert not first_ids & {row["id"] for row in second.data["results"]}

    def test_archived_pages_are_hidden_by_default(self, api_key_client, workspace, page_project, create_user):
        _make_page(page_project, create_user, name="Live")
        _make_page(page_project, create_user, name="Archived", archived_at="2024-01-01")

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert [row["name"] for row in response.data["results"]] == ["Live"]

    def test_include_archived_returns_archived_pages(self, api_key_client, workspace, page_project, create_user):
        _make_page(page_project, create_user, name="Live")
        _make_page(page_project, create_user, name="Archived", archived_at="2024-01-01")

        response = api_key_client.get(_list_url(workspace.slug, page_project.id), {"include_archived": "true"})

        assert {row["name"] for row in response.data["results"]} == {"Live", "Archived"}

    def test_another_members_private_page_is_not_listed(
        self, api_key_client, workspace, page_project, create_user, other_user
    ):
        _make_page(page_project, create_user, name="Shared", access=0)
        _make_page(page_project, other_user, name="Theirs", access=1)

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert [row["name"] for row in response.data["results"]] == ["Shared"]

    def test_own_private_page_is_listed(self, api_key_client, workspace, page_project, create_user):
        _make_page(page_project, create_user, name="Mine", access=1)

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert [row["name"] for row in response.data["results"]] == ["Mine"]

    def test_pages_of_another_project_are_not_listed(self, api_key_client, workspace, page_project, create_user):
        other_project = Project.objects.create(
            name="Other", identifier="OTH", workspace=workspace, created_by=create_user
        )
        ProjectMember.objects.create(project=other_project, member=create_user, role=20, is_active=True)
        _make_page(other_project, create_user, name="Elsewhere")

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert response.data["results"] == []

    def test_non_member_gets_403(self, api_key_client, workspace, create_user):
        stranger_project = Project.objects.create(
            name="No Access", identifier="NOACC", workspace=workspace, created_by=create_user
        )
        _make_page(stranger_project, create_user, name="Secret")

        response = api_key_client.get(_list_url(workspace.slug, stranger_project.id))

        assert response.status_code == http_status.HTTP_403_FORBIDDEN

    def test_archived_project_exposes_no_pages(self, api_key_client, workspace, page_project, create_user):
        _make_page(page_project, create_user, name="Live")
        page_project.archived_at = timezone.now()
        page_project.save()

        response = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["results"] == []


@pytest.mark.contract
@pytest.mark.django_db
class TestPageDetail:
    def test_member_reads_page_with_all_three_renderings(self, api_key_client, workspace, page_project, create_user):
        page = _make_page(
            page_project,
            create_user,
            name="Conventions",
            html="<h1>Rules</h1><ul><li><p>one</p></li></ul>",
        )

        response = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["name"] == "Conventions"
        assert response.data["description_html"] == "<h1>Rules</h1><ul><li><p>one</p></li></ul>"
        assert response.data["description_stripped"] == "Rulesone"
        assert response.data["description_markdown"] == "# Rules\n\n- one"

    def test_detail_payload_is_list_metadata_plus_the_body_fields(
        self, api_key_client, workspace, page_project, create_user
    ):
        page = _make_page(page_project, create_user)

        detail = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))
        listing = api_key_client.get(_list_url(workspace.slug, page_project.id))

        assert set(detail.data) - set(listing.data["results"][0]) == {
            "description_html",
            "description_stripped",
            "description_markdown",
        }

    def test_another_members_private_page_is_404_not_403(self, api_key_client, workspace, page_project, other_user):
        """404, so the API never confirms somebody else's private page exists."""
        page = _make_page(page_project, other_user, name="Theirs", access=1)

        response = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    def test_own_private_page_is_readable(self, api_key_client, workspace, page_project, create_user):
        page = _make_page(page_project, create_user, name="Mine", access=1)

        response = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["name"] == "Mine"

    def test_archived_page_is_readable_by_id(self, api_key_client, workspace, page_project, create_user):
        """The list hides archived pages; a direct read still resolves one."""
        page = _make_page(page_project, create_user, name="Archived", archived_at="2024-01-01")

        response = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["archived_at"] == "2024-01-01"

    def test_page_from_another_project_is_404(self, api_key_client, workspace, page_project, create_user):
        other_project = Project.objects.create(
            name="Other", identifier="OTH2", workspace=workspace, created_by=create_user
        )
        ProjectMember.objects.create(project=other_project, member=create_user, role=20, is_active=True)
        page = _make_page(other_project, create_user, name="Elsewhere")

        response = api_key_client.get(_detail_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    def test_non_member_gets_403(self, api_key_client, workspace, create_user):
        stranger_project = Project.objects.create(
            name="No Access", identifier="NOACC2", workspace=workspace, created_by=create_user
        )
        page = _make_page(stranger_project, create_user, name="Secret")

        response = api_key_client.get(_detail_url(workspace.slug, stranger_project.id, page.id))

        assert response.status_code == http_status.HTTP_403_FORBIDDEN

    def test_unknown_page_id_is_404(self, api_key_client, workspace, page_project):
        response = api_key_client.get(
            _detail_url(workspace.slug, page_project.id, "550e8400-e29b-41d4-a716-446655440000")
        )

        assert response.status_code == http_status.HTTP_404_NOT_FOUND
