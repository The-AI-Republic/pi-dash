# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the prompt-label CRUD + attach/detach endpoints
(PDASHOSS01-71)."""

from __future__ import annotations

import pytest
from django.urls import reverse
from rest_framework.test import APIClient

from pi_dash.db.models import User, WorkspaceMember
from pi_dash.prompting.models import PromptLabel, PromptLabelAssignment


@pytest.fixture
def member_user(db):
    return User.objects.create(
        username="member", email="member@example.com", first_name="Mem", last_name="Ber"
    )


@pytest.fixture
def member_client(db, workspace, member_user):
    """A non-admin (role=15) workspace member."""
    WorkspaceMember.objects.create(workspace=workspace, member=member_user, role=15)
    client = APIClient()
    client.force_authenticate(user=member_user)
    return client


@pytest.fixture
def outsider_client(db):
    outsider = User.objects.create(
        username="outsider", email="out@example.com", first_name="Out", last_name="Sider"
    )
    client = APIClient()
    client.force_authenticate(user=outsider)
    return client


def _list_url(workspace):
    return reverse("prompting:prompt-label-list", kwargs={"slug": workspace.slug})


def _detail_url(workspace, label_id):
    return reverse(
        "prompting:prompt-label-detail", kwargs={"slug": workspace.slug, "label_id": label_id}
    )


def _assign_url(workspace, label_id):
    return reverse(
        "prompting:prompt-label-assignments",
        kwargs={"slug": workspace.slug, "label_id": label_id},
    )


def _make_label(session_client, workspace, name="Important", color="#ff0000"):
    resp = session_client.post(
        _list_url(workspace), {"name": name, "color": color}, format="json"
    )
    assert resp.status_code == 201, resp.data
    return resp.data


# ----------------------------------------------------------------------
# Create / list
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_admin_creates_label(session_client, workspace):
    resp = session_client.post(
        _list_url(workspace), {"name": "Tone", "color": "#00ff00"}, format="json"
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["name"] == "Tone"
    assert resp.data["color"] == "#00ff00"
    assert resp.data["targets"] == []
    assert PromptLabel.objects.filter(workspace=workspace, name="Tone").exists()


@pytest.mark.contract
def test_create_defaults_color_when_missing(session_client, workspace):
    resp = session_client.post(_list_url(workspace), {"name": "NoColor"}, format="json")
    assert resp.status_code == 201, resp.data
    assert resp.data["color"] == "#6b7280"


@pytest.mark.contract
def test_create_requires_name(session_client, workspace):
    resp = session_client.post(_list_url(workspace), {"name": "   "}, format="json")
    assert resp.status_code == 400


@pytest.mark.contract
def test_create_duplicate_name_conflicts(session_client, workspace):
    _make_label(session_client, workspace, name="Dup")
    resp = session_client.post(_list_url(workspace), {"name": "Dup"}, format="json")
    assert resp.status_code == 409


@pytest.mark.contract
def test_member_cannot_create_label(member_client, workspace):
    resp = member_client.post(_list_url(workspace), {"name": "Nope"}, format="json")
    assert resp.status_code == 403


@pytest.mark.contract
def test_outsider_cannot_read_labels(outsider_client, workspace):
    resp = outsider_client.get(_list_url(workspace))
    assert resp.status_code == 403


@pytest.mark.contract
def test_member_can_read_labels(member_client, session_client, workspace):
    _make_label(session_client, workspace, name="Visible")
    resp = member_client.get(_list_url(workspace))
    assert resp.status_code == 200
    names = [label_row["name"] for label_row in resp.data["labels"]]
    assert "Visible" in names


# ----------------------------------------------------------------------
# Update / delete
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_admin_updates_label(session_client, workspace):
    label = _make_label(session_client, workspace, name="Old", color="#111111")
    resp = session_client.patch(
        _detail_url(workspace, label["id"]), {"name": "New", "color": "#222222"}, format="json"
    )
    assert resp.status_code == 200, resp.data
    assert resp.data["name"] == "New"
    assert resp.data["color"] == "#222222"


@pytest.mark.contract
def test_member_cannot_update_label(session_client, member_client, workspace):
    label = _make_label(session_client, workspace)
    resp = member_client.patch(
        _detail_url(workspace, label["id"]), {"name": "Hacked"}, format="json"
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_admin_deletes_label_cascades_assignments(session_client, workspace):
    label = _make_label(session_client, workspace)
    session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "section", "target_key": "intro"},
        format="json",
    )
    assert PromptLabelAssignment.objects.filter(label_id=label["id"]).exists()
    resp = session_client.delete(_detail_url(workspace, label["id"]))
    assert resp.status_code == 204
    assert not PromptLabel.objects.filter(id=label["id"]).exists()
    assert not PromptLabelAssignment.objects.filter(label_id=label["id"]).exists()


# ----------------------------------------------------------------------
# Attach / detach
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_attach_to_section_and_receipt(session_client, workspace):
    label = _make_label(session_client, workspace)
    r1 = session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "section", "target_key": "intro"},
        format="json",
    )
    assert r1.status_code == 200, r1.data
    r2 = session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "receipt", "target_key": "coding-task"},
        format="json",
    )
    assert r2.status_code == 200, r2.data
    targets = {(t["target_type"], t["target_key"]) for t in r2.data["targets"]}
    assert ("section", "intro") in targets
    assert ("receipt", "coding-task") in targets


@pytest.mark.contract
def test_attach_is_idempotent(session_client, workspace):
    label = _make_label(session_client, workspace)
    payload = {"target_type": "section", "target_key": "intro"}
    session_client.post(_assign_url(workspace, label["id"]), payload, format="json")
    resp = session_client.post(_assign_url(workspace, label["id"]), payload, format="json")
    assert resp.status_code == 200
    assert (
        PromptLabelAssignment.objects.filter(
            label_id=label["id"], target_type="section", target_key="intro"
        ).count()
        == 1
    )


@pytest.mark.contract
def test_attach_unknown_target_type_400(session_client, workspace):
    label = _make_label(session_client, workspace)
    resp = session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "bogus", "target_key": "intro"},
        format="json",
    )
    assert resp.status_code == 400


@pytest.mark.contract
def test_attach_unknown_section_404(session_client, workspace):
    label = _make_label(session_client, workspace)
    resp = session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "section", "target_key": "does-not-exist"},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.contract
def test_attach_unknown_receipt_kind_404(session_client, workspace):
    label = _make_label(session_client, workspace)
    resp = session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "receipt", "target_key": "not-a-kind"},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.contract
def test_member_cannot_attach(session_client, member_client, workspace):
    label = _make_label(session_client, workspace)
    resp = member_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "section", "target_key": "intro"},
        format="json",
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_detach(session_client, workspace):
    label = _make_label(session_client, workspace)
    session_client.post(
        _assign_url(workspace, label["id"]),
        {"target_type": "section", "target_key": "intro"},
        format="json",
    )
    resp = session_client.delete(
        _assign_url(workspace, label["id"]) + "?target_type=section&target_key=intro"
    )
    assert resp.status_code == 204
    assert not PromptLabelAssignment.objects.filter(label_id=label["id"]).exists()


@pytest.mark.contract
def test_detach_missing_assignment_404(session_client, workspace):
    label = _make_label(session_client, workspace)
    resp = session_client.delete(
        _assign_url(workspace, label["id"]) + "?target_type=section&target_key=intro"
    )
    assert resp.status_code == 404


@pytest.mark.contract
def test_labels_scoped_to_workspace(session_client, workspace, create_user):
    """A label created in one workspace must not leak into another."""
    from pi_dash.db.models import Workspace

    other = Workspace.objects.create(
        name="Other", slug="other-workspace", owner=create_user
    )
    WorkspaceMember.objects.create(workspace=other, member=create_user, role=20)
    _make_label(session_client, workspace, name="OnlyHere")

    resp = session_client.get(_list_url(other))
    assert resp.status_code == 200
    assert resp.data["labels"] == []
