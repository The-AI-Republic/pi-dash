# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the ``/api/v1/`` project page write endpoints.

These are how an agent (``pidash page create|update|archive``, the MCP page
tool) contributes to a project wiki. Under test is the caller's view — who
may write what, and which error comes back — plus the one property that is
invisible from the response but decides whether a write survives: the body
is stored as HTML, JSON *and* the collaborative-document binary together,
via the live server, or not at all.

The live server is faked at the HTTP boundary (``requests.post`` inside
:mod:`pi_dash.utils.live_document`), so the helper's request/response
handling is exercised too.
"""

import base64
import uuid
from unittest import mock

import pytest
import requests
from rest_framework import status as http_status

from pi_dash.bgtasks.page_transaction_task import page_transaction
from pi_dash.bgtasks.page_version_task import track_page_version
from pi_dash.db.models import Page, PageLog, PageVersion, Project, ProjectMember, ProjectPage, User

LIVE_URL = "http://live.test/live/"
FAKE_BINARY = b"\x01\x02\x03\x04fake-yjs-state"


@pytest.fixture
def page_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Page Write Project",
        identifier="PW",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(project=project, member=create_user, role=15, is_active=True)
    return project


@pytest.fixture
def other_user(db, page_project):
    user = User.objects.create(email="page-owner@example.com", username="page-owner", first_name="Owner")
    ProjectMember.objects.create(project=page_project, member=user, role=15, is_active=True)
    return user


class FakeLive:
    """Stands in for ``POST {LIVE_URL}/convert-document/``."""

    def __init__(self):
        self.calls = []
        self.fail_with = None

    def __call__(self, url, json=None, timeout=None, **kwargs):
        self.calls.append({"url": url, "json": json})
        if self.fail_with is not None:
            raise self.fail_with
        response = mock.Mock(status_code=200)
        response.json.return_value = {
            "description_json": {"type": "doc", "content": [{"type": "paragraph"}]},
            "description_binary": base64.b64encode(FAKE_BINARY).decode("ascii"),
            # The real server returns the editor's re-serialisation.
            "description_html": json["description_html"].replace("<p>", '<p class="editor-paragraph-block">'),
        }
        return response


@pytest.fixture
def live(settings):
    settings.LIVE_URL = LIVE_URL
    fake = FakeLive()
    with mock.patch("pi_dash.utils.live_document.requests.post", side_effect=fake):
        yield fake


@pytest.fixture
def tasks():
    """Background tasks are recorded, not queued; ``run_inline`` executes them."""
    with (
        mock.patch("pi_dash.api.views.page.page_transaction") as transaction_task,
        mock.patch("pi_dash.api.views.page.track_page_version") as version_task,
    ):
        yield mock.Mock(page_transaction=transaction_task.delay, track_page_version=version_task.delay)


def _make_page(project, owner, *, name="A page", html="<p>body</p>", binary=None, **fields):
    page = Page.objects.create(
        name=name,
        description_html=html,
        description_binary=binary,
        owned_by=owner,
        workspace=project.workspace,
        **fields,
    )
    ProjectPage.objects.create(workspace=project.workspace, project=project, page=page)
    return page


def _list_url(slug, project_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/"


def _detail_url(slug, project_id, page_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/"


def _archive_url(slug, project_id, page_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/"


@pytest.mark.contract
@pytest.mark.django_db
class TestPageCreate:
    def test_member_creates_page_from_markdown(self, api_key_client, workspace, page_project, create_user, live, tasks):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Conventions", "description_markdown": "# Rules\n\nUse **snake_case**."},
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert response.data["name"] == "Conventions"
        assert response.data["owned_by"] == create_user.id
        assert "# Rules" in response.data["description_markdown"]
        page = Page.objects.get(pk=response.data["id"])
        assert ProjectPage.objects.filter(page=page, project=page_project).exists()
        # The markdown was converted server-side before reaching the live server.
        sent = live.calls[0]["json"]
        assert "<h1>Rules</h1>" in sent["description_html"]
        assert "<strong>snake_case</strong>" in sent["description_html"]
        assert sent["variant"] == "document"
        assert sent["title"] == "Conventions"
        assert "description_binary" not in sent

    def test_create_persists_html_json_and_binary_together(
        self, api_key_client, workspace, page_project, live, tasks
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Stored", "description_markdown": "hello"},
            format="json",
        )

        page = Page.objects.get(pk=response.data["id"])
        assert bytes(page.description_binary) == FAKE_BINARY
        assert page.description_json == {"type": "doc", "content": [{"type": "paragraph"}]}
        # The editor-serialised HTML returned by the live server is what is stored.
        assert page.description_html == '<p class="editor-paragraph-block">hello</p>'

    def test_create_accepts_html_body(self, api_key_client, workspace, page_project, live, tasks):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Html", "description_html": "<h2>Title</h2><script>alert(1)</script>"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        sent = live.calls[0]["json"]["description_html"]
        assert "<h2>Title</h2>" in sent
        assert "<script>" not in sent

    def test_markdown_and_html_together_is_400(self, api_key_client, workspace, page_project, live, tasks):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Both", "description_markdown": "a", "description_html": "<p>a</p>"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST
        assert live.calls == []

    def test_name_is_required(self, api_key_client, workspace, page_project, live, tasks):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id), {"description_markdown": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST

    def test_create_with_parent_and_private_access(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        parent = _make_page(page_project, create_user, name="Parent")

        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Child", "parent": str(parent.id), "access": 1},
            format="json",
        )

        assert response.status_code == http_status.HTTP_201_CREATED
        assert response.data["parent"] == parent.id
        assert response.data["access"] == Page.PRIVATE_ACCESS

    def test_parent_from_another_project_is_rejected(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        elsewhere = Project.objects.create(name="Elsewhere", identifier="ELSE", workspace=workspace)
        ProjectMember.objects.create(project=elsewhere, member=create_user, role=15, is_active=True)
        foreign_parent = _make_page(elsewhere, create_user, name="Foreign")

        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Child", "parent": str(foreign_parent.id)},
            format="json",
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST

    def test_non_member_gets_403(self, api_key_client, workspace, create_user, live, tasks):
        stranger_project = Project.objects.create(name="No Access", identifier="NOACC", workspace=workspace)

        response = api_key_client.post(
            _list_url(workspace.slug, stranger_project.id), {"name": "Nope"}, format="json"
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        assert not Page.objects.filter(name="Nope").exists()

    def test_guest_cannot_create(self, api_key_client, workspace, page_project, create_user, live, tasks):
        ProjectMember.objects.filter(project=page_project, member=create_user).update(role=5)

        response = api_key_client.post(_list_url(workspace.slug, page_project.id), {"name": "Nope"}, format="json")

        assert response.status_code == http_status.HTTP_403_FORBIDDEN

    def test_archived_project_is_rejected(self, api_key_client, workspace, page_project, live, tasks):
        from django.utils import timezone

        page_project.archived_at = timezone.now()
        page_project.save()

        response = api_key_client.post(_list_url(workspace.slug, page_project.id), {"name": "Late"}, format="json")

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert not Page.objects.filter(name="Late").exists()


@pytest.mark.contract
@pytest.mark.django_db
class TestLiveConversionUnavailable:
    """A body the editor would silently discard must never be reported as saved."""

    def test_create_fails_loudly_when_live_url_is_unset(self, api_key_client, workspace, page_project, settings, tasks):
        settings.LIVE_URL = None

        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id),
            {"name": "Lost", "description_markdown": "text"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_503_SERVICE_UNAVAILABLE
        assert "LIVE_URL" in response.data["error"]
        assert not Page.objects.filter(name="Lost").exists()
        tasks.page_transaction.assert_not_called()

    def test_update_fails_loudly_when_live_server_is_down(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, html="<p>original</p>", binary=b"\x00old-state")
        live.fail_with = requests.ConnectionError("refused")

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id),
            {"description_markdown": "new text", "name": "Renamed"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_503_SERVICE_UNAVAILABLE
        page.refresh_from_db()
        assert page.description_html == "<p>original</p>"
        assert page.name == "A page"
        assert bytes(page.description_binary) == b"\x00old-state"
        tasks.track_page_version.assert_not_called()

    def test_live_server_error_status_is_503(self, api_key_client, workspace, page_project, settings, tasks):
        settings.LIVE_URL = LIVE_URL
        with mock.patch(
            "pi_dash.utils.live_document.requests.post", return_value=mock.Mock(status_code=500)
        ):
            response = api_key_client.post(
                _list_url(workspace.slug, page_project.id), {"name": "Err"}, format="json"
            )

        assert response.status_code == http_status.HTTP_503_SERVICE_UNAVAILABLE


@pytest.mark.contract
@pytest.mark.django_db
class TestPageUpdate:
    def test_body_update_persists_all_three_formats_on_top_of_existing_binary(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, html="<p>old</p>", binary=b"\x00old-state")

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id),
            {"description_markdown": "- [x] done\n- [ ] todo"},
            format="json",
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["description_markdown"] == "- [x] done\n- [ ] todo"
        sent = live.calls[0]["json"]
        # The change is applied onto the current document, not a fresh one.
        assert base64.b64decode(sent["description_binary"]) == b"\x00old-state"
        page.refresh_from_db()
        assert bytes(page.description_binary) == FAKE_BINARY
        assert page.description_json == {"type": "doc", "content": [{"type": "paragraph"}]}
        assert 'data-type="taskList"' in page.description_html

    def test_member_can_edit_another_members_public_page(
        self, api_key_client, workspace, page_project, other_user, live, tasks
    ):
        page = _make_page(page_project, other_user)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"description_markdown": "edit"}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK

    def test_rename_rewrites_the_title_inside_the_document(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, html="<p>keep</p>", binary=b"\x00old-state")

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "New name"}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["name"] == "New name"
        sent = live.calls[0]["json"]
        assert sent["title"] == "New name"
        assert sent["description_html"] == "<p>keep</p>"
        # A rename is not a body write: no new version, no transaction.
        tasks.track_page_version.assert_not_called()

    def test_rename_of_a_never_opened_page_skips_the_live_server(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "Fresh"}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert live.calls == []

    def test_parent_can_be_set_and_cleared(self, api_key_client, workspace, page_project, create_user, live, tasks):
        parent = _make_page(page_project, create_user, name="Parent")
        page = _make_page(page_project, create_user, name="Child")
        url = _detail_url(workspace.slug, page_project.id, page.id)

        assert api_key_client.patch(url, {"parent": str(parent.id)}, format="json").data["parent"] == parent.id
        assert api_key_client.patch(url, {"parent": None}, format="json").data["parent"] is None

    def test_page_cannot_be_nested_under_its_own_descendant(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, name="Top")
        child = _make_page(page_project, create_user, name="Child", parent=page)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"parent": str(child.id)}, format="json"
        )

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST

    def test_empty_patch_is_400(self, api_key_client, workspace, page_project, create_user, live, tasks):
        page = _make_page(page_project, create_user)

        response = api_key_client.patch(_detail_url(workspace.slug, page_project.id, page.id), {}, format="json")

        assert response.status_code == http_status.HTTP_400_BAD_REQUEST

    def test_locked_page_rejects_body_writes(self, api_key_client, workspace, page_project, create_user, live, tasks):
        page = _make_page(page_project, create_user, is_locked=True)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"description_markdown": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["error_message"] == "PAGE_LOCKED"
        assert response.data["error_code"] == 4701
        assert live.calls == []

    def test_locked_page_rejects_metadata_writes(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, is_locked=True)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "Sneaky"}, format="json"
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["error_message"] == "PAGE_LOCKED"
        page.refresh_from_db()
        assert page.name == "A page"

    def test_lock_applied_during_the_live_conversion_wins(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, html="<p>old</p>", binary=b"\x00old-state")
        convert = live.__call__

        def lock_then_convert(url, json=None, timeout=None, **kwargs):
            # A human locks the page while the live server is converting.
            Page.objects.filter(pk=page.pk).update(is_locked=True)
            return convert(url, json=json, timeout=timeout, **kwargs)

        with mock.patch("pi_dash.utils.live_document.requests.post", side_effect=lock_then_convert):
            response = api_key_client.patch(
                _detail_url(workspace.slug, page_project.id, page.id), {"description_markdown": "x"}, format="json"
            )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["error_message"] == "PAGE_LOCKED"
        page.refresh_from_db()
        assert page.is_locked is True
        assert page.description_html == "<p>old</p>"
        tasks.track_page_version.assert_not_called()

    def test_archived_page_rejects_body_writes(self, api_key_client, workspace, page_project, create_user, live, tasks):
        page = _make_page(page_project, create_user, archived_at="2024-01-01")

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"description_markdown": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["error_message"] == "PAGE_ARCHIVED"
        assert response.data["error_code"] == 4702

    def test_archived_page_still_accepts_metadata_writes(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, archived_at="2024-01-01")

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "Renamed"}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK

    def test_owner_can_change_access(self, api_key_client, workspace, page_project, create_user, live, tasks):
        page = _make_page(page_project, create_user)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"access": 1}, format="json"
        )

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["access"] == Page.PRIVATE_ACCESS

    def test_non_owner_cannot_change_access(self, api_key_client, workspace, page_project, other_user, live, tasks):
        page = _make_page(page_project, other_user)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"access": 1}, format="json"
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        page.refresh_from_db()
        assert page.access == Page.PUBLIC_ACCESS

    def test_another_members_private_page_is_404(
        self, api_key_client, workspace, page_project, other_user, live, tasks
    ):
        page = _make_page(page_project, other_user, access=Page.PRIVATE_ACCESS)

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    def test_non_member_gets_403(self, api_key_client, workspace, create_user, live, tasks):
        stranger_project = Project.objects.create(name="No Access", identifier="NOACC", workspace=workspace)
        page = _make_page(stranger_project, create_user)

        response = api_key_client.patch(
            _detail_url(workspace.slug, stranger_project.id, page.id), {"name": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_403_FORBIDDEN

    def test_page_in_archived_project_is_not_writable(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        from django.utils import timezone

        page = _make_page(page_project, create_user)
        page_project.archived_at = timezone.now()
        page_project.save()

        response = api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"name": "x"}, format="json"
        )

        assert response.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.contract
@pytest.mark.django_db
class TestBodyWriteBookkeeping:
    """Agent edits must show up in page history and mention/backlink
    bookkeeping exactly like editor edits do."""

    def test_body_write_fires_transaction_and_version_tasks(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, html="<p>old</p>")

        api_key_client.patch(
            _detail_url(workspace.slug, page_project.id, page.id), {"description_markdown": "new"}, format="json"
        )

        page.refresh_from_db()
        tasks.page_transaction.assert_called_once_with(
            new_description_html=page.description_html, old_description_html="<p>old</p>", page_id=page.id
        )
        tasks.track_page_version.assert_called_once()
        assert tasks.track_page_version.call_args.kwargs["page_id"] == page.id
        assert tasks.track_page_version.call_args.kwargs["user_id"] == create_user.id

    def test_body_write_creates_a_page_version_and_a_page_transaction(
        self, api_key_client, workspace, page_project, create_user, live
    ):
        """Runs the real tasks inline rather than asserting they were queued."""
        page = _make_page(page_project, create_user, html="<p>old</p>")
        mention = (
            f'<p>Ask <mention-component id="{uuid.uuid4()}" entity_identifier="{create_user.id}" '
            'entity_name="user_mention"></mention-component></p>'
        )
        with (
            mock.patch("pi_dash.api.views.page.page_transaction.delay", side_effect=page_transaction),
            mock.patch("pi_dash.api.views.page.track_page_version.delay", side_effect=track_page_version),
        ):
            response = api_key_client.patch(
                _detail_url(workspace.slug, page_project.id, page.id), {"description_html": mention}, format="json"
            )

        assert response.status_code == http_status.HTTP_200_OK
        version = PageVersion.objects.get(page=page)
        assert "mention-component" in version.description_html
        assert version.owned_by_id == create_user.id
        assert PageLog.objects.filter(page=page, entity_name="user_mention").exists()

    def test_create_with_body_records_a_version(self, api_key_client, workspace, page_project, live, tasks):
        response = api_key_client.post(
            _list_url(workspace.slug, page_project.id), {"name": "V", "description_markdown": "v1"}, format="json"
        )

        tasks.page_transaction.assert_called_once()
        assert tasks.page_transaction.call_args.kwargs["old_description_html"] is None
        assert tasks.track_page_version.call_args.kwargs["page_id"] == response.data["id"]


@pytest.mark.contract
@pytest.mark.django_db
class TestPageArchive:
    def test_owner_archives_and_unarchives_with_descendants(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        page = _make_page(page_project, create_user, name="Top")
        child = _make_page(page_project, create_user, name="Child", parent=page)
        url = _archive_url(workspace.slug, page_project.id, page.id)

        archived = api_key_client.post(url)

        assert archived.status_code == http_status.HTTP_200_OK
        assert archived.data["archived_at"] is not None
        child.refresh_from_db()
        assert child.archived_at is not None

        restored = api_key_client.delete(url)

        assert restored.status_code == http_status.HTTP_200_OK
        assert restored.data["archived_at"] is None
        child.refresh_from_db()
        assert child.archived_at is None

    def test_unarchiving_under_an_archived_parent_moves_the_page_to_the_top(
        self, api_key_client, workspace, page_project, create_user, live, tasks
    ):
        parent = _make_page(page_project, create_user, name="Parent", archived_at="2024-01-01")
        page = _make_page(page_project, create_user, name="Child", parent=parent, archived_at="2024-01-01")

        response = api_key_client.delete(_archive_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.data["parent"] is None
        assert response.data["archived_at"] is None

    def test_member_cannot_archive_someone_elses_page(
        self, api_key_client, workspace, page_project, other_user, live, tasks
    ):
        page = _make_page(page_project, other_user)

        response = api_key_client.post(_archive_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
        page.refresh_from_db()
        assert page.archived_at is None

    def test_admin_can_archive_someone_elses_page(
        self, api_key_client, workspace, page_project, create_user, other_user, live, tasks
    ):
        ProjectMember.objects.filter(project=page_project, member=create_user).update(role=20)
        page = _make_page(page_project, other_user)

        response = api_key_client.post(_archive_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_200_OK

    def test_locked_page_cannot_be_archived(self, api_key_client, workspace, page_project, create_user, live, tasks):
        page = _make_page(page_project, create_user, is_locked=True)

        response = api_key_client.post(_archive_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_409_CONFLICT
        assert response.data["error_message"] == "PAGE_LOCKED"

    def test_another_members_private_page_is_404(
        self, api_key_client, workspace, page_project, other_user, live, tasks
    ):
        page = _make_page(page_project, other_user, access=Page.PRIVATE_ACCESS)

        response = api_key_client.post(_archive_url(workspace.slug, page_project.id, page.id))

        assert response.status_code == http_status.HTTP_404_NOT_FOUND

    def test_non_member_gets_403(self, api_key_client, workspace, create_user, live, tasks):
        stranger_project = Project.objects.create(name="No Access", identifier="NOACC", workspace=workspace)
        page = _make_page(stranger_project, create_user)

        response = api_key_client.post(_archive_url(workspace.slug, stranger_project.id, page.id))

        assert response.status_code == http_status.HTTP_403_FORBIDDEN
