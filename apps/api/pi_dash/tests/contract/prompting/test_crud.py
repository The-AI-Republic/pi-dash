# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the prompt-template CRUD surface."""

import pytest
from crum import impersonate
from django.urls import reverse

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.prompting.models import PromptTemplate
from pi_dash.prompting.seed import seed_default_template


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def admin_user(create_user, workspace):
    # ``create_user`` is already the workspace owner+admin via the ``workspace``
    # fixture (it creates a WorkspaceMember with role=20). Return it unchanged.
    return create_user


@pytest.fixture
def member_user(db, workspace):
    member = User.objects.create(
        email="member@pi-dash.so", username="member-user"
    )
    member.set_password("p")
    member.save()
    WorkspaceMember.objects.create(
        workspace=workspace, member=member, role=15, is_active=True
    )
    return member


@pytest.fixture
def outsider(db):
    other = User.objects.create(
        email="outsider@pi-dash.so", username="outsider-user"
    )
    other.set_password("p")
    other.save()
    return other


def _list_url(ws: Workspace) -> str:
    return reverse(
        "prompting:prompt-template-list-create", kwargs={"slug": ws.slug}
    )


def _detail_url(ws: Workspace, template_id) -> str:
    return reverse(
        "prompting:prompt-template-detail",
        kwargs={"slug": ws.slug, "template_id": template_id},
    )


def _archive_url(ws: Workspace, template_id) -> str:
    return reverse(
        "prompting:prompt-template-archive",
        kwargs={"slug": ws.slug, "template_id": template_id},
    )


@pytest.mark.contract
def test_list_returns_global_default_when_no_override(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    response = api_client.get(_list_url(workspace))
    assert response.status_code == 200, response.content
    body = response.json()
    assert len(body) == 1
    assert body[0]["is_global_default"] is True
    assert body[0]["can_edit"] is False
    assert body[0]["workspace"] is None


@pytest.mark.contract
def test_list_forbidden_to_non_members(
    seeded, api_client, workspace, outsider
):
    api_client.force_authenticate(user=outsider)
    response = api_client.get(_list_url(workspace))
    assert response.status_code == 403


@pytest.mark.contract
def test_member_can_list_but_not_create(
    seeded, api_client, workspace, member_user
):
    api_client.force_authenticate(user=member_user)
    list_response = api_client.get(_list_url(workspace))
    assert list_response.status_code == 200

    create_response = api_client.post(_list_url(workspace), {}, format="json")
    assert create_response.status_code == 403


@pytest.mark.contract
def test_admin_create_copies_global_default_body(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    response = api_client.post(_list_url(workspace), {}, format="json")
    assert response.status_code == 201, response.content
    body = response.json()
    assert body["workspace"] == str(workspace.id)
    assert body["is_global_default"] is False
    assert body["can_edit"] is True
    assert body["version"] == 1
    # Body must be non-empty — copied from the global default.
    assert len(body["body"]) > 100


@pytest.mark.contract
def test_admin_create_rejects_when_override_exists(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    first = api_client.post(_list_url(workspace), {}, format="json")
    assert first.status_code == 201

    second = api_client.post(
        _list_url(workspace), {"body": "custom body"}, format="json"
    )
    assert second.status_code == 409
    assert "existing_id" in second.json()


@pytest.mark.contract
def test_admin_patch_bumps_version_and_stamps_updater(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    created = api_client.post(
        _list_url(workspace), {"body": "Hello {{ issue.title }}"}, format="json"
    )
    template_id = created.json()["id"]

    response = api_client.patch(
        _detail_url(workspace, template_id),
        {"body": "Hi {{ issue.title }}"},
        format="json",
    )
    assert response.status_code == 200, response.content
    body = response.json()
    assert body["body"] == "Hi {{ issue.title }}"
    assert body["version"] == 2
    assert body["updated_by"] == str(admin_user.id)


@pytest.mark.contract
def test_patch_rejects_global_default(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    global_default = PromptTemplate.objects.filter(workspace__isnull=True).first()
    response = api_client.patch(
        _detail_url(workspace, global_default.id),
        {"body": "tampered"},
        format="json",
    )
    assert response.status_code == 403


@pytest.mark.contract
def test_patch_rejects_jinja_syntax_errors(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    created = api_client.post(
        _list_url(workspace), {"body": "valid {{ issue.title }}"}, format="json"
    )
    template_id = created.json()["id"]

    response = api_client.patch(
        _detail_url(workspace, template_id),
        {"body": "broken {% if %}"},
        format="json",
    )
    assert response.status_code == 400
    assert "body" in response.json()


@pytest.mark.contract
def test_archive_flips_is_active_and_creates_can_retry(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    created = api_client.post(_list_url(workspace), {}, format="json")
    template_id = created.json()["id"]

    archive = api_client.post(_archive_url(workspace, template_id))
    assert archive.status_code == 200, archive.content
    assert archive.json()["is_active"] is False

    # After archiving, the list should show only the global default again.
    listed = api_client.get(_list_url(workspace)).json()
    assert len(listed) == 1
    assert listed[0]["is_global_default"] is True

    # And the admin can create a fresh override.
    retry = api_client.post(_list_url(workspace), {}, format="json")
    assert retry.status_code == 201


@pytest.mark.contract
def test_archive_rejects_global_default(
    seeded, api_client, workspace, admin_user
):
    api_client.force_authenticate(user=admin_user)
    global_default = PromptTemplate.objects.filter(workspace__isnull=True).first()
    response = api_client.post(_archive_url(workspace, global_default.id))
    assert response.status_code == 404  # cannot archive what doesn't belong to this workspace


@pytest.mark.contract
def test_member_cannot_archive(
    seeded, api_client, workspace, admin_user, member_user
):
    api_client.force_authenticate(user=admin_user)
    created = api_client.post(_list_url(workspace), {}, format="json")
    template_id = created.json()["id"]

    api_client.force_authenticate(user=member_user)
    response = api_client.post(_archive_url(workspace, template_id))
    assert response.status_code == 403
