# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the prompt-section CRUD endpoints (design §7.2)."""

from __future__ import annotations

import pytest
from django.urls import reverse
from rest_framework.test import APIClient

from pi_dash.db.models import User, WorkspaceMember
from pi_dash.prompting import registry
from pi_dash.prompting.models import PromptSectionOverride


def _retier(monkeypatch, key, tier):
    """Replace a registry section with a copy at a different governance tier,
    so the tier gate can be exercised without hard-coding which real section
    happens to carry it."""
    original = registry.get_section(key)
    monkeypatch.setitem(
        registry.REGISTRY,
        key,
        registry.PromptSection(
            key=original.key,
            title=original.title,
            customizable=tier,
            default_body=original.default_body,
        ),
    )


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


def _list_url(workspace, kind="coding-task", scope=None):
    url = reverse("prompting:prompt-section-list", kwargs={"slug": workspace.slug})
    q = f"?kind={kind}"
    if scope:
        q += f"&scope={scope}"
    return url + q


def _detail_url(workspace, section_key):
    return reverse(
        "prompting:prompt-section-detail",
        kwargs={"slug": workspace.slug, "section_key": section_key},
    )


# ----------------------------------------------------------------------
# List / read
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_list_returns_ordered_sections(session_client, workspace):
    resp = session_client.get(_list_url(workspace))
    assert resp.status_code == 200
    keys = [s["key"] for s in resp.data["sections"]]
    assert keys[0] == "intro" and keys[-1] == "ending-run"
    assert all(s["source"] == "default" for s in resp.data["sections"])


@pytest.mark.contract
def test_list_unknown_kind_400(session_client, workspace):
    resp = session_client.get(_list_url(workspace, kind="bogus"))
    assert resp.status_code == 400


@pytest.mark.contract
def test_list_forbidden_for_outsider(outsider_client, workspace):
    resp = outsider_client.get(_list_url(workspace))
    assert resp.status_code == 403


# ----------------------------------------------------------------------
# Write (workspace scope = admin)
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_admin_puts_workspace_override(session_client, workspace):
    resp = session_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "workspace", "body": "Custom workspace guidance."},
        format="json",
    )
    assert resp.status_code == 200, resp.data
    assert PromptSectionOverride.objects.filter(
        workspace=workspace, user__isnull=True, section_key="implementation", is_active=True
    ).exists()
    # list now reflects the override
    resp2 = session_client.get(_list_url(workspace, scope="workspace"))
    impl = next(s for s in resp2.data["sections"] if s["key"] == "implementation")
    assert impl["source"] == "workspace"
    assert impl["body"] == "Custom workspace guidance."


@pytest.mark.contract
def test_member_cannot_put_workspace_override(member_client, workspace):
    resp = member_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "workspace", "body": "nope"},
        format="json",
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_member_can_put_own_user_override(member_client, workspace, member_user):
    resp = member_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "user", "body": "My personal guidance."},
        format="json",
    )
    assert resp.status_code == 200, resp.data
    assert PromptSectionOverride.objects.filter(
        workspace=workspace, user=member_user, section_key="implementation", is_active=True
    ).exists()


@pytest.mark.contract
def test_put_locked_section_forbidden(session_client, workspace):
    resp = session_client.put(
        _detail_url(workspace, "pidash-cli"),
        {"scope": "workspace", "body": "hack the cli docs"},
        format="json",
    )
    assert resp.status_code == 403


# ----------------------------------------------------------------------
# Governance tier: workspace-only (admin overrides, members cannot)
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_workspace_tier_admin_can_override(session_client, workspace, monkeypatch):
    _retier(monkeypatch, "implementation", registry.CUSTOMIZABLE_WORKSPACE)
    resp = session_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "workspace", "body": "Org-wide guidance."},
        format="json",
    )
    assert resp.status_code == 200, resp.data


@pytest.mark.contract
def test_workspace_tier_member_cannot_personally_override(member_client, workspace, monkeypatch):
    _retier(monkeypatch, "implementation", registry.CUSTOMIZABLE_WORKSPACE)
    resp = member_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "user", "body": "my personal copy"},
        format="json",
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_list_exposes_tier_and_capabilities(session_client, workspace, monkeypatch):
    _retier(monkeypatch, "implementation", registry.CUSTOMIZABLE_WORKSPACE)
    resp = session_client.get(_list_url(workspace))
    assert resp.status_code == 200
    by_key = {s["key"]: s for s in resp.data["sections"]}

    # Every section carries the new diff + capability fields.
    impl = by_key["implementation"]
    assert "default_body" in impl
    assert impl["customizable"] == "workspace"
    assert impl["editable_at_workspace"] is True
    assert impl["editable_at_personal"] is False

    locked = by_key["pidash-cli"]
    assert locked["editable_at_workspace"] is False
    assert locked["editable_at_personal"] is False

    open_ = by_key["analyze-and-scope"]
    assert open_["editable_at_workspace"] is True
    assert open_["editable_at_personal"] is True


@pytest.mark.contract
def test_put_invalid_jinja_rejected(session_client, workspace):
    resp = session_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "workspace", "body": "{% if x %}unclosed"},
        format="json",
    )
    assert resp.status_code == 400


@pytest.mark.contract
def test_put_unknown_variable_rejected(session_client, workspace):
    resp = session_client.put(
        _detail_url(workspace, "implementation"),
        {"scope": "workspace", "body": "{{ totally_unknown }}"},
        format="json",
    )
    assert resp.status_code == 400


@pytest.mark.contract
def test_put_unknown_section_404(session_client, workspace):
    resp = session_client.put(
        _detail_url(workspace, "no-such-section"),
        {"scope": "workspace", "body": "x"},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.contract
def test_put_bumps_version_in_place(session_client, workspace):
    url = _detail_url(workspace, "implementation")
    r1 = session_client.put(url, {"scope": "workspace", "body": "v1"}, format="json")
    r2 = session_client.put(url, {"scope": "workspace", "body": "v2"}, format="json")
    assert r1.data["version"] == 1
    assert r2.data["version"] == 2
    # still exactly one active row
    assert (
        PromptSectionOverride.objects.filter(
            workspace=workspace, user__isnull=True, section_key="implementation", is_active=True
        ).count()
        == 1
    )


# ----------------------------------------------------------------------
# Delete / revert
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_delete_reverts_to_default(session_client, workspace):
    url = _detail_url(workspace, "implementation")
    session_client.put(url, {"scope": "workspace", "body": "custom"}, format="json")
    resp = session_client.delete(url + "?scope=workspace")
    assert resp.status_code == 204
    # back to default in the list
    listed = session_client.get(_list_url(workspace, scope="workspace"))
    impl = next(s for s in listed.data["sections"] if s["key"] == "implementation")
    assert impl["source"] == "default"


@pytest.mark.contract
def test_delete_no_override_404(session_client, workspace):
    resp = session_client.delete(_detail_url(workspace, "implementation") + "?scope=workspace")
    assert resp.status_code == 404
