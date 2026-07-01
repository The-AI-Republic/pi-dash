# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the compiled-template and preview endpoints (§7.2)."""

from __future__ import annotations

import pytest
from crum import impersonate
from django.urls import reverse
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.db.models import Issue, Project, State, User
from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding
from pi_dash.prompting import recipes, registry
from pi_dash.prompting.models import PromptSectionOverride


def _section_of_tier(kind, tier):
    """First section key in ``kind``'s recipe with the given customizable tier."""
    for key in recipes.recipe_for(kind):
        if registry.get_section(key).customizable == tier:
            return key
    return None


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web", identifier="WEB", workspace=workspace, created_by=create_user
        )


@pytest.fixture
def issue(db, workspace, project, create_user):
    with impersonate(create_user):
        state = State.objects.create(name="Todo", project=project, group="unstarted")
        return Issue.objects.create(
            name="Blue button",
            workspace=workspace,
            project=project,
            state=state,
            created_by=create_user,
        )


@pytest.fixture
def binding(db, workspace, project, create_user):
    scheduler = Scheduler.objects.create(
        workspace=workspace, slug="audit", name="Audit", prompt="Scan repo."
    )
    return SchedulerBinding.objects.create(
        scheduler=scheduler,
        project=project,
        workspace=workspace,
        dtstart=timezone.now(),
        actor=create_user,
    )


def _compiled_url(workspace, kind, scope=None):
    url = reverse("prompting:prompt-compiled", kwargs={"slug": workspace.slug, "kind": kind})
    return url + (f"?scope={scope}" if scope else "")


def _preview_url(workspace, kind):
    return reverse("prompting:prompt-preview", kwargs={"slug": workspace.slug, "kind": kind})


# ----------------------------------------------------------------------
# Compiled
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_compiled_returns_template(session_client, workspace):
    resp = session_client.get(_compiled_url(workspace, "coding-task"))
    assert resp.status_code == 200
    assert "{{ issue.identifier }}" in resp.data["template_body"]
    assert resp.data["kind"] == "coding-task"
    # The per-section breakdown is served by the section-list endpoint, not here.
    assert "sections" not in resp.data


@pytest.mark.contract
def test_compiled_unknown_kind_400(session_client, workspace):
    resp = session_client.get(_compiled_url(workspace, "bogus"))
    assert resp.status_code == 400


@pytest.mark.contract
def test_compiled_dual_compilation_when_user_override_exists(
    session_client, workspace, create_user
):
    # create_user has a personal override → compiled (scope=user) returns both
    # the user template and the workspace-only "automatic runs" template.
    PromptSectionOverride.objects.create(
        workspace=workspace, user=create_user, section_key="implementation", body="MINE ONLY"
    )
    resp = session_client.get(_compiled_url(workspace, "coding-task", scope="user"))
    assert resp.status_code == 200
    assert "MINE ONLY" in resp.data["template_body"]
    assert "automatic_template_body" in resp.data
    assert "MINE ONLY" not in resp.data["automatic_template_body"]


@pytest.mark.contract
def test_compiled_no_dual_when_no_user_override(session_client, workspace):
    resp = session_client.get(_compiled_url(workspace, "coding-task", scope="user"))
    assert "automatic_template_body" not in resp.data


# ----------------------------------------------------------------------
# Preview
# ----------------------------------------------------------------------


@pytest.mark.contract
def test_preview_issue_renders(session_client, workspace, issue):
    resp = session_client.post(
        _preview_url(workspace, "coding-task"), {"issue_id": str(issue.id)}, format="json"
    )
    assert resp.status_code == 200, resp.data
    assert f"{issue.project.identifier}-{issue.sequence_id}" in resp.data["prompt"]


@pytest.mark.contract
def test_preview_review_kind_against_issue(session_client, workspace, issue):
    resp = session_client.post(
        _preview_url(workspace, "review"), {"issue_id": str(issue.id)}, format="json"
    )
    assert resp.status_code == 200, resp.data
    # the requested kind wins → review prompt content
    assert "reviewing the work product" in resp.data["prompt"]


@pytest.mark.contract
def test_preview_scheduler_renders(session_client, workspace, binding):
    resp = session_client.post(
        _preview_url(workspace, "scheduler"), {"binding_id": str(binding.id)}, format="json"
    )
    assert resp.status_code == 200, resp.data
    assert "Scan repo." in resp.data["prompt"]


@pytest.mark.contract
def test_preview_missing_issue_id_400(session_client, workspace):
    resp = session_client.post(_preview_url(workspace, "coding-task"), {}, format="json")
    assert resp.status_code == 400


@pytest.mark.contract
def test_preview_scheduler_without_binding_id_400(session_client, workspace):
    resp = session_client.post(_preview_url(workspace, "scheduler"), {}, format="json")
    assert resp.status_code == 400


@pytest.mark.contract
def test_preview_workspace_scope_forbidden_for_non_admin(db, workspace, issue):
    from pi_dash.db.models import WorkspaceMember

    member = User.objects.create(
        username="m2", email="m2@example.com", first_name="M", last_name="2"
    )
    WorkspaceMember.objects.create(workspace=workspace, member=member, role=15)
    client = APIClient()
    client.force_authenticate(user=member)
    # Default scope is workspace — a member may not preview the workspace default.
    resp = client.post(
        _preview_url(workspace, "coding-task"), {"issue_id": str(issue.id)}, format="json"
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_preview_user_scope_allowed_for_member(db, workspace, issue):
    from pi_dash.db.models import WorkspaceMember

    member = User.objects.create(
        username="m3", email="m3@example.com", first_name="M", last_name="3"
    )
    WorkspaceMember.objects.create(workspace=workspace, member=member, role=15)
    client = APIClient()
    client.force_authenticate(user=member)
    # A member may preview their own (user-scope) composition to check a draft.
    resp = client.post(
        _preview_url(workspace, "coding-task"),
        {"issue_id": str(issue.id), "scope": "user"},
        format="json",
    )
    assert resp.status_code == 200
    assert resp.data["prompt"]


@pytest.mark.contract
def test_preview_draft_renders_overridable_section(session_client, workspace, issue):
    key = _section_of_tier("coding-task", registry.CUSTOMIZABLE_OVERRIDABLE)
    if key is None:
        pytest.skip("no overridable section in the coding-task recipe")
    resp = session_client.post(
        _preview_url(workspace, "coding-task"),
        {"issue_id": str(issue.id), "section_key": key, "body": "DRAFT-PREVIEW-MARKER"},
        format="json",
    )
    assert resp.status_code == 200
    assert "DRAFT-PREVIEW-MARKER" in resp.data["prompt"]


@pytest.mark.contract
def test_preview_draft_locked_section_forbidden(session_client, workspace, issue):
    key = _section_of_tier("coding-task", registry.CUSTOMIZABLE_LOCKED)
    if key is None:
        pytest.skip("no locked section in the coding-task recipe")
    resp = session_client.post(
        _preview_url(workspace, "coding-task"),
        {"issue_id": str(issue.id), "section_key": key, "body": "SHOULD-BE-BLOCKED"},
        format="json",
    )
    assert resp.status_code == 403


@pytest.mark.contract
def test_preview_draft_missing_body_400(session_client, workspace, issue):
    key = recipes.recipe_for("coding-task")[0]
    resp = session_client.post(
        _preview_url(workspace, "coding-task"),
        {"issue_id": str(issue.id), "section_key": key},
        format="json",
    )
    assert resp.status_code == 400
