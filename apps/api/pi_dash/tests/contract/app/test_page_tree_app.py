# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for nested (child) pages on the web app's page endpoints
(``/api/workspaces/<slug>/projects/<id>/pages/``, session auth).

The default list stays top-level only so the old web app is unchanged;
``?parent=`` lists one page's children, a search spans every depth, and
detail / create / update work for child pages (PDASHOSS01-270).
"""

import pytest
from django.utils import timezone
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.db.models import Page, Project, ProjectMember, ProjectPage, User


@pytest.fixture
def page_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Page Tree Project",
        identifier="PT",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def other_user(db):
    return User.objects.create(email="tree-other@example.com", username="tree-other", first_name="Other")


def _make_page(project, owner, *, name="A page", access=0, archived_at=None, parent=None):
    page = Page.objects.create(
        name=name,
        description_html="<p>body</p>",
        owned_by=owner,
        access=access,
        archived_at=archived_at,
        parent=parent,
        workspace=project.workspace,
    )
    ProjectPage.objects.create(workspace=project.workspace, project=project, page=page)
    return page


def _list_url(slug, project_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/pages/"


def _detail_url(slug, project_id, page_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/"


def _names(response):
    return sorted(row["name"] for row in response.data)


@pytest.mark.contract
@pytest.mark.django_db
class TestPageTreeList:
    def test_default_list_is_top_level_only(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        _make_page(page_project, create_user, name="Child", parent=parent)
        _make_page(page_project, create_user, name="Other root")

        response = session_client.get(_list_url(workspace.slug, page_project.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert _names(response) == ["Other root", "Parent"]

    def test_parent_param_lists_children(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        child = _make_page(page_project, create_user, name="Child", parent=parent)
        _make_page(page_project, create_user, name="Grandchild", parent=child)
        _make_page(page_project, create_user, name="Root")

        response = session_client.get(_list_url(workspace.slug, page_project.id), {"parent": str(parent.id)})

        assert response.status_code == http_status.HTTP_200_OK
        assert _names(response) == ["Child"]
        assert response.data[0]["parent_id"] == str(parent.id)
        assert response.data[0]["sub_pages_count"] == 1

    def test_invalid_parent_param_is_rejected(self, session_client, workspace, page_project):
        response = session_client.get(_list_url(workspace.slug, page_project.id), {"parent": "not-a-uuid"})

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Invalid parent page id"}

    def test_search_includes_child_pages(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Roadmap")
        _make_page(page_project, create_user, name="Roadmap Q4", parent=parent)
        _make_page(page_project, create_user, name="Unrelated", parent=parent)

        response = session_client.get(_list_url(workspace.slug, page_project.id), {"search": "roadmap"})

        assert _names(response) == ["Roadmap", "Roadmap Q4"]

    def test_rows_carry_parent_id_and_sub_pages_count(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        _make_page(page_project, create_user, name="Child 1", parent=parent)
        _make_page(page_project, create_user, name="Child 2", parent=parent)
        _make_page(page_project, create_user, name="Archived child", parent=parent, archived_at=timezone.now())
        leaf = _make_page(page_project, create_user, name="Leaf")

        response = session_client.get(_list_url(workspace.slug, page_project.id))

        rows = {row["name"]: row for row in response.data}
        assert rows["Parent"]["parent_id"] is None
        assert rows["Parent"]["sub_pages_count"] == 2
        assert rows["Leaf"]["sub_pages_count"] == 0
        assert str(leaf.id) == str(rows["Leaf"]["id"])

    def test_archived_page_counts_its_archived_children(self, session_client, workspace, page_project, create_user):
        archived_at = timezone.now()
        parent = _make_page(page_project, create_user, name="Old", archived_at=archived_at)
        _make_page(page_project, create_user, name="Old child", parent=parent, archived_at=archived_at)

        response = session_client.get(_list_url(workspace.slug, page_project.id))

        rows = {row["name"]: row for row in response.data}
        assert rows["Old"]["sub_pages_count"] == 1

    def test_someone_elses_private_child_is_invisible(
        self, session_client, workspace, page_project, create_user, other_user
    ):
        ProjectMember.objects.create(project=page_project, member=other_user, role=20, is_active=True)
        parent = _make_page(page_project, create_user, name="Parent")
        _make_page(page_project, other_user, name="Their private child", parent=parent, access=1)
        _make_page(page_project, other_user, name="Their public child", parent=parent, access=0)

        top = session_client.get(_list_url(workspace.slug, page_project.id))
        children = session_client.get(_list_url(workspace.slug, page_project.id), {"parent": str(parent.id)})

        assert top.data[0]["sub_pages_count"] == 1
        assert _names(children) == ["Their public child"]


@pytest.mark.contract
@pytest.mark.django_db
class TestPageTreeDetail:
    def test_child_page_detail(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        child = _make_page(page_project, create_user, name="Child", parent=parent)

        response = session_client.get(_detail_url(workspace.slug, page_project.id, child.id), {"track_visit": "false"})

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["name"] == "Child"
        assert response.data["parent"] == parent.id
        assert response.data["parent_id"] == str(parent.id)
        assert response.data["sub_pages_count"] == 0

    def test_parent_detail_counts_children(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        _make_page(page_project, create_user, name="Child", parent=parent)

        response = session_client.get(_detail_url(workspace.slug, page_project.id, parent.id), {"track_visit": "false"})

        assert response.data["sub_pages_count"] == 1

    def test_someone_elses_private_child_detail_is_forbidden(
        self, session_client, workspace, page_project, create_user, other_user
    ):
        ProjectMember.objects.create(project=page_project, member=other_user, role=20, is_active=True)
        parent = _make_page(page_project, create_user, name="Parent")
        child = _make_page(page_project, other_user, name="Private", parent=parent, access=1)

        response = session_client.get(_detail_url(workspace.slug, page_project.id, child.id), {"track_visit": "false"})

        # ProjectPagePermission refuses another member's private page.
        assert response.status_code == http_status.HTTP_403_FORBIDDEN


@pytest.mark.contract
@pytest.mark.django_db
class TestPageTreeWrites:
    def test_create_child_page(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")

        response = session_client.post(
            _list_url(workspace.slug, page_project.id), {"name": "Child", "parent": str(parent.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert response.data["parent_id"] == str(parent.id)
        assert Page.objects.get(pk=response.data["id"]).parent_id == parent.id

    def test_create_under_archived_parent_is_rejected(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent", archived_at=timezone.now())

        response = session_client.post(
            _list_url(workspace.slug, page_project.id), {"name": "Child", "parent": str(parent.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Parent page is archived"}
        assert not Page.objects.filter(name="Child").exists()

    def test_create_under_invisible_parent_is_rejected(
        self, session_client, workspace, page_project, create_user, other_user
    ):
        ProjectMember.objects.create(project=page_project, member=other_user, role=20, is_active=True)
        parent = _make_page(page_project, other_user, name="Their private page", access=1)

        response = session_client.post(
            _list_url(workspace.slug, page_project.id), {"name": "Child", "parent": str(parent.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Parent page not found in this project"}

    def test_create_under_page_in_another_project_is_rejected(
        self, session_client, workspace, page_project, project, create_user
    ):
        elsewhere = _make_page(project, create_user, name="Elsewhere")

        response = session_client.post(
            _list_url(workspace.slug, page_project.id), {"name": "Child", "parent": str(elsewhere.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Parent page not found in this project"}

    def test_move_page_under_another(self, session_client, workspace, page_project, create_user):
        target = _make_page(page_project, create_user, name="Target")
        page = _make_page(page_project, create_user, name="Moved")

        response = session_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": str(target.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        page.refresh_from_db()
        assert page.parent_id == target.id

    def test_move_page_to_top_level(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        page = _make_page(page_project, create_user, name="Child", parent=parent)

        response = session_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": None}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        page.refresh_from_db()
        assert page.parent_id is None

    def test_move_under_own_descendant_is_rejected(self, session_client, workspace, page_project, create_user):
        page = _make_page(page_project, create_user, name="Page")
        child = _make_page(page_project, create_user, name="Child", parent=page)
        grandchild = _make_page(page_project, create_user, name="Grandchild", parent=child)

        response = session_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": str(grandchild.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "A page cannot be nested under itself"}
        page.refresh_from_db()
        assert page.parent_id is None

    def test_move_under_itself_is_rejected(self, session_client, workspace, page_project, create_user):
        page = _make_page(page_project, create_user, name="Page")

        response = session_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": str(page.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "A page cannot be nested under itself"}

    def test_move_under_archived_page_is_rejected(self, session_client, workspace, page_project, create_user):
        target = _make_page(page_project, create_user, name="Target", archived_at=timezone.now())
        page = _make_page(page_project, create_user, name="Page")

        response = session_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": str(target.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert response.data == {"error": "Parent page is archived"}


@pytest.mark.contract
@pytest.mark.django_db
class TestPageTreeLifecycleUnchanged:
    def test_archive_cascades_to_descendants(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent")
        child = _make_page(page_project, create_user, name="Child", parent=parent)
        grandchild = _make_page(page_project, create_user, name="Grandchild", parent=child)

        response = session_client.post(f"{_detail_url(workspace.slug, page_project.id, parent.id)}archive/")

        assert response.status_code == http_status.HTTP_200_OK
        child.refresh_from_db()
        grandchild.refresh_from_db()
        assert child.archived_at is not None
        assert grandchild.archived_at is not None

    def test_unarchive_under_archived_parent_detaches(self, session_client, workspace, page_project, create_user):
        archived_at = timezone.now()
        parent = _make_page(page_project, create_user, name="Parent", archived_at=archived_at)
        child = _make_page(page_project, create_user, name="Child", parent=parent, archived_at=archived_at)

        response = session_client.delete(f"{_detail_url(workspace.slug, page_project.id, child.id)}archive/")

        assert response.status_code == http_status.HTTP_204_NO_CONTENT
        child.refresh_from_db()
        assert child.parent_id is None
        assert child.archived_at is None

    def test_delete_moves_children_to_top_level(self, session_client, workspace, page_project, create_user):
        parent = _make_page(page_project, create_user, name="Parent", archived_at=timezone.now())
        child = _make_page(page_project, create_user, name="Child", parent=parent)

        response = session_client.delete(_detail_url(workspace.slug, page_project.id, parent.id))

        assert response.status_code == http_status.HTTP_204_NO_CONTENT
        child.refresh_from_db()
        assert child.parent_id is None


@pytest.mark.contract
@pytest.mark.django_db
def test_guest_counts_only_own_child_pages(workspace, page_project, create_user, other_user):
    """A guest without ``guest_view_all_features`` sees only pages they own,
    so their child counts include only those too."""
    ProjectMember.objects.create(project=page_project, member=other_user, role=5, is_active=True)
    parent = _make_page(page_project, other_user, name="Guest parent")
    _make_page(page_project, other_user, name="Guest child", parent=parent)
    _make_page(page_project, create_user, name="Admin child", parent=parent)
    client = APIClient()
    client.force_authenticate(user=other_user)

    response = client.get(_list_url(workspace.slug, page_project.id))

    assert [row["name"] for row in response.data] == ["Guest parent"]
    assert response.data[0]["sub_pages_count"] == 1
