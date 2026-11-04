# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for moving a page to another project.

``POST .../pages/<page_id>/move/`` exists on the public ``/api/v1/`` surface
(``pidash page move``) and on the web app's API. Both share
:mod:`pi_dash.utils.page_move`; the v1 tests cover the rules in depth and the
app tests check that the web route applies the same ones.
"""

import uuid

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import (
    Label,
    Page,
    PageLabel,
    PageLog,
    PageVersion,
    Project,
    ProjectMember,
    ProjectPage,
    User,
    UserFavorite,
    Workspace,
)


@pytest.fixture
def source(db, workspace, create_user):
    project = Project.objects.create(name="Source", identifier="SRC", workspace=workspace, created_by=create_user)
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def target(db, workspace, create_user):
    project = Project.objects.create(name="Target", identifier="TGT", workspace=workspace, created_by=create_user)
    ProjectMember.objects.create(project=project, member=create_user, role=15, is_active=True)
    return project


def _make_page(project, owner, *, name="A page", **fields):
    page = Page.objects.create(
        name=name,
        description_html='<p data-id="x1" style="">body</p>',
        owned_by=owner,
        workspace=project.workspace,
        **fields,
    )
    ProjectPage.objects.create(workspace=project.workspace, project=project, page=page)
    return page


def _v1(slug, project_id, page_id, suffix=""):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/{suffix}"


def _app_move(slug, project_id, page_id):
    return f"/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/move/"


def _linked_projects(page):
    return set(ProjectPage.objects.filter(page=page).values_list("project_id", flat=True))


@pytest.mark.contract
@pytest.mark.django_db
class TestPageMoveV1:
    def test_move_relinks_page_and_keeps_its_identity(self, api_key_client, workspace, source, target, create_user):
        other = User.objects.create(email="owner@example.com", username="owner")
        ProjectMember.objects.create(project=source, member=other, role=15, is_active=True)
        page = _make_page(source, other, name="Wiki")
        label = Label.objects.create(name="docs", workspace=workspace, project=source)
        PageLabel.objects.create(page=page, label=label, workspace=workspace)
        PageVersion.objects.create(page=page, workspace=workspace, owned_by=other, description_html="<p>v1</p>")
        before = Page.objects.values("id", "owned_by_id", "description_html", "description_binary", "name").get(
            pk=page.pk
        )

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["id"] == page.id
        assert response.data["owned_by"] == other.id
        assert _linked_projects(page) == {target.id}
        after = Page.objects.values("id", "owned_by_id", "description_html", "description_binary", "name").get(
            pk=page.pk
        )
        assert after == before
        page.refresh_from_db()
        assert page.moved_to_project == target.id
        assert page.updated_by_id == create_user.id
        assert list(PageLabel.objects.filter(page=page).values_list("label_id", flat=True)) == [label.id]
        assert PageVersion.objects.filter(page=page).count() == 1

        # Readable under the target, gone from the source.
        assert api_key_client.get(_v1(workspace.slug, target.id, page.id)).status_code == 200
        assert api_key_client.get(_v1(workspace.slug, source.id, page.id)).status_code == 404
        source_ids = [
            p["id"]
            for p in api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/projects/{source.id}/pages/").data[
                "results"
            ]
        ]
        target_ids = [
            p["id"]
            for p in api_key_client.get(f"/api/v1/workspaces/{workspace.slug}/projects/{target.id}/pages/").data[
                "results"
            ]
        ]
        assert str(page.id) not in map(str, source_ids)
        assert str(page.id) in map(str, target_ids)

    def test_response_matches_get_under_target(self, api_key_client, workspace, source, target, create_user):
        page = _make_page(source, create_user)

        moved = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )
        fetched = api_key_client.get(_v1(workspace.slug, target.id, page.id))

        assert moved.json() == fetched.json()

    def test_children_move_with_parent(self, api_key_client, workspace, source, target, create_user):
        page = _make_page(source, create_user, name="Top")
        child = _make_page(source, create_user, name="Child", parent=page)
        grandchild = _make_page(source, create_user, name="Grandchild", parent=child)
        sibling = _make_page(source, create_user, name="Sibling")

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 200
        for moved in (page, child, grandchild):
            assert _linked_projects(moved) == {target.id}
            moved.refresh_from_db()
            assert moved.moved_to_project == target.id
        child.refresh_from_db()
        grandchild.refresh_from_db()
        assert child.parent_id == page.id
        assert grandchild.parent_id == child.id
        assert _linked_projects(sibling) == {source.id}

    def test_child_moved_alone_loses_its_parent(self, api_key_client, workspace, source, target, create_user):
        parent = _make_page(source, create_user, name="Parent")
        child = _make_page(source, create_user, name="Child", parent=parent)
        grandchild = _make_page(source, create_user, name="Grandchild", parent=child)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, child.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 200
        assert response.data["parent"] is None
        grandchild.refresh_from_db()
        assert grandchild.parent_id == child.id
        assert _linked_projects(parent) == {source.id}
        assert _linked_projects(grandchild) == {target.id}

    def test_source_favorites_are_removed_and_move_is_logged(
        self, api_key_client, workspace, source, target, create_user
    ):
        page = _make_page(source, create_user)
        UserFavorite.objects.create(
            user=create_user, workspace=workspace, project=source, entity_type="page", entity_identifier=page.id
        )

        api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert not UserFavorite.objects.filter(entity_identifier=page.id).exists()
        log = PageLog.objects.get(page=page, entity_name="moved_to_project")
        assert log.entity_identifier == target.id
        assert log.entity_type == "project"
        assert log.created_by_id == create_user.id

    @pytest.mark.parametrize(
        "body,message",
        [
            ({}, "project_id is required"),
            ({"project_id": ""}, "project_id is required"),
            ({"project_id": "not-a-uuid"}, "project_id must be a valid project id"),
        ],
    )
    def test_invalid_body_is_400(self, api_key_client, workspace, source, create_user, body, message):
        page = _make_page(source, create_user)

        response = api_key_client.post(_v1(workspace.slug, source.id, page.id, "move/"), body, format="json")

        assert response.status_code == 400
        assert response.json() == {"error": message}

    def test_same_project_is_400(self, api_key_client, workspace, source, create_user):
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(source.id)}, format="json"
        )

        assert response.status_code == 400
        assert response.json() == {"error": "The page is already in this project"}

    def test_target_in_another_workspace_is_400(self, api_key_client, workspace, source, create_user):
        other_ws = Workspace.objects.create(name="Other", slug="other-ws", owner=create_user)
        foreign = Project.objects.create(name="Foreign", identifier="FOR", workspace=other_ws)
        ProjectMember.objects.create(project=foreign, member=create_user, role=20, is_active=True, workspace=other_ws)
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(foreign.id)}, format="json"
        )

        assert response.status_code == 400
        assert response.json() == {"error": "The target project is not in this workspace"}
        assert _linked_projects(page) == {source.id}

    def test_unknown_target_is_400(self, api_key_client, workspace, source, create_user):
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(uuid.uuid4())}, format="json"
        )

        assert response.status_code == 400

    def test_archived_target_is_400(self, api_key_client, workspace, source, target, create_user):
        Project.objects.filter(pk=target.pk).update(archived_at="2024-01-01T00:00:00Z")
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 400
        assert response.json() == {"error": "The target project is archived"}

    def test_locked_page_is_400(self, api_key_client, workspace, source, target, create_user):
        page = _make_page(source, create_user, is_locked=True)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 400
        assert response.json() == {"error": "Page is locked", "error_code": 4701, "error_message": "PAGE_LOCKED"}
        assert _linked_projects(page) == {source.id}

    def test_member_of_source_is_403(self, api_key_client, workspace, source, target, create_user):
        ProjectMember.objects.filter(project=source, member=create_user).update(role=15)
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 403
        assert response.json() == {"error": "Only a project admin can move a page to another project"}

    @pytest.mark.parametrize("role,active", [(5, True), (15, False), (None, None)])
    def test_no_write_access_in_target_is_403(
        self, api_key_client, workspace, source, target, create_user, role, active
    ):
        if role is None:
            ProjectMember.objects.filter(project=target, member=create_user).delete(soft=False)
        else:
            ProjectMember.objects.filter(project=target, member=create_user).update(role=role, is_active=active)
        page = _make_page(source, create_user)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 403
        assert response.json() == {"error": "You need write access to the target project to move a page into it"}
        assert _linked_projects(page) == {source.id}

    def test_another_members_private_page_is_404(self, api_key_client, workspace, source, target, create_user):
        other = User.objects.create(email="owner@example.com", username="owner")
        page = _make_page(source, other, access=Page.PRIVATE_ACCESS)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 404

    def test_page_already_linked_to_target_keeps_one_link(self, api_key_client, workspace, source, target, create_user):
        page = _make_page(source, create_user)
        ProjectPage.objects.create(workspace=workspace, project=target, page=page)

        response = api_key_client.post(
            _v1(workspace.slug, source.id, page.id, "move/"), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 200
        assert list(ProjectPage.objects.filter(page=page).values_list("project_id", flat=True)) == [target.id]


@pytest.mark.contract
@pytest.mark.django_db
class TestPageMoveApp:
    def test_move_returns_204_and_relinks(self, session_client, workspace, source, target, create_user):
        page = _make_page(source, create_user)
        child = _make_page(source, create_user, parent=page)

        response = session_client.post(
            _app_move(workspace.slug, source.id, page.id), {"project_id": str(target.id)}, format="json"
        )

        assert response.status_code == 204
        assert _linked_projects(page) == {target.id}
        assert _linked_projects(child) == {target.id}

    def test_same_rules_as_v1(self, session_client, workspace, source, target, create_user):
        page = _make_page(source, create_user, is_locked=True)

        locked = session_client.post(
            _app_move(workspace.slug, source.id, page.id), {"project_id": str(target.id)}, format="json"
        )
        same = session_client.post(
            _app_move(workspace.slug, source.id, page.id), {"project_id": str(source.id)}, format="json"
        )
        missing = session_client.post(
            _app_move(workspace.slug, source.id, uuid.uuid4()), {"project_id": str(target.id)}, format="json"
        )

        assert locked.status_code == 400
        assert locked.json()["error_message"] == "PAGE_LOCKED"
        assert same.status_code == 400
        assert missing.status_code == 404
        assert missing.json() == {"error": "Page not found"}
