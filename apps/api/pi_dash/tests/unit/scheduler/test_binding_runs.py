# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the scheduler-binding run-history surface (PDASHOSS01-223).

Covers the project-scoped binding-runs endpoint (permissions, pagination,
binding isolation, private-runner gate), the binding *detail* serializer's
``resolved_prompt`` / ``run_count`` additions, the ``scheduler_binding`` /
``scheduler_binding_detail`` fields on the AgentRun serializer, the
project-member visibility grant on the run detail view, and the
``?scheduler_binding=`` filter on the global runs list.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from crum import impersonate
from django.contrib.auth import get_user_model
from django.utils import timezone
from rest_framework.test import APIRequestFactory, force_authenticate

from pi_dash.app.views.scheduler.views import (
    ProjectSchedulerBindingDetailEndpoint,
    ProjectSchedulerBindingRunsEndpoint,
)
from pi_dash.db.models import Project, ProjectMember, Scheduler, SchedulerBinding, WorkspaceMember
from pi_dash.runner.models import AgentRun, Pod
from pi_dash.runner.views.runs import AgentRunDetailEndpoint, AgentRunListEndpoint

User = get_user_model()


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        p = Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )
    # create_user is a project admin so allow_permission(PROJECT) passes.
    ProjectMember.objects.create(project=p, workspace=workspace, member=create_user, role=20)
    return p


@pytest.fixture
def scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="nightly-audit",
            name="Nightly Audit",
            prompt="Audit the project.",
            color="#10b981",
        )


def _make_second_binding(project, workspace, user):
    """A second binding needs its own definition — one active binding per
    (scheduler, project) is enforced by a DB constraint."""
    with impersonate(user):
        other_scheduler = Scheduler.objects.create(
            workspace=workspace,
            slug="weekly-report",
            name="Weekly Report",
            prompt="Report.",
            color="#3b82f6",
        )
    return _make_binding(other_scheduler, project, workspace, user)


def _make_binding(scheduler, project, workspace, user, **overrides):
    defaults = dict(
        scheduler=scheduler,
        project=project,
        workspace=workspace,
        dtstart=timezone.now() - timedelta(days=1),
        rrule="FREQ=DAILY",
        tzid="UTC",
        enabled=True,
        actor=user,
    )
    defaults.update(overrides)
    with impersonate(user):
        return SchedulerBinding.objects.create(**defaults)


@pytest.fixture
def binding(scheduler, project, workspace, create_user):
    return _make_binding(scheduler, project, workspace, create_user)


@pytest.fixture
def default_pod(project):
    return Pod.objects.get(project=project, is_default=True)


def _make_run(binding, pod, user, **overrides):
    defaults = dict(
        workspace=binding.workspace,
        created_by=user,
        pod=pod,
        prompt="",
        scheduler_binding=binding,
        status="completed",
    )
    defaults.update(overrides)
    return AgentRun.objects.create(**defaults)


def _get_binding_runs(workspace, project, binding, user, query=None):
    factory = APIRequestFactory()
    request = factory.get(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}"
        f"/scheduler-bindings/{binding.id}/runs/",
        query or {},
    )
    force_authenticate(request, user=user)
    view = ProjectSchedulerBindingRunsEndpoint.as_view()
    return view(
        request,
        slug=workspace.slug,
        project_id=str(project.id),
        binding_id=str(binding.id),
    )


# ---------------------------------------------------------------------------
# Binding-runs endpoint: isolation, ordering, pagination
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_binding_runs_returns_only_this_bindings_runs_newest_first(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    other_binding = _make_second_binding(project, workspace, create_user)
    first = _make_run(binding, default_pod, create_user)
    second = _make_run(binding, default_pod, create_user)
    _make_run(other_binding, default_pod, create_user)  # must not appear

    res = _get_binding_runs(workspace, project, binding, create_user)
    assert res.status_code == 200
    ids = [r["id"] for r in res.data["results"]]
    # Newest first: `second` was created after `first`.
    assert ids == [str(second.id), str(first.id)]
    assert res.data["total_count"] == 2


@pytest.mark.unit
def test_binding_runs_pagination_envelope(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    for _ in range(3):
        _make_run(binding, default_pod, create_user)

    res = _get_binding_runs(
        workspace, project, binding, create_user, {"page": "2", "per_page": "2"}
    )
    assert res.status_code == 200
    assert res.data["page"] == 2
    assert res.data["per_page"] == 2
    assert res.data["total_count"] == 3
    assert res.data["total_pages"] == 2
    assert res.data["count"] == 1
    assert len(res.data["results"]) == 1


@pytest.mark.unit
def test_binding_runs_rows_carry_scheduler_binding_detail(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    _make_run(binding, default_pod, create_user)

    res = _get_binding_runs(workspace, project, binding, create_user)
    assert res.status_code == 200
    row = res.data["results"][0]
    assert row["scheduler_binding"] == binding.id
    assert row["scheduler_binding_detail"] == {
        "id": str(binding.id),
        "scheduler_name": "Nightly Audit",
        "scheduler_slug": "nightly-audit",
        "project": str(project.id),
    }


# ---------------------------------------------------------------------------
# Binding-runs endpoint: permissions
# ---------------------------------------------------------------------------


def _make_member(workspace, project, *, email, workspace_role=15, project_role=None):
    user = User.objects.create(email=email, username=email)
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=workspace_role)
    if project_role is not None:
        ProjectMember.objects.create(
            project=project, workspace=workspace, member=user, role=project_role
        )
    return user


@pytest.mark.unit
def test_binding_runs_readable_by_project_guest(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    guest = _make_member(
        workspace, project, email="guest@example.com", workspace_role=5, project_role=5
    )
    _make_run(binding, default_pod, create_user)

    res = _get_binding_runs(workspace, project, binding, guest)
    assert res.status_code == 200
    assert res.data["total_count"] == 1


@pytest.mark.unit
def test_binding_runs_forbidden_for_workspace_member_outside_project(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    outsider = _make_member(workspace, project, email="outsider@example.com", project_role=None)

    res = _get_binding_runs(workspace, project, binding, outsider)
    assert res.status_code == 403


@pytest.mark.unit
def test_binding_runs_404_when_binding_belongs_to_another_project(
    db, workspace, project, scheduler, binding, create_user
):
    with impersonate(create_user):
        other_project = Project.objects.create(
            name="API",
            identifier="API",
            workspace=workspace,
            created_by=create_user,
        )
    ProjectMember.objects.create(
        project=other_project, workspace=workspace, member=create_user, role=20
    )

    # Ask the *other* project's URL for this project's binding.
    res = _get_binding_runs(workspace, other_project, binding, create_user)
    assert res.status_code == 404


@pytest.mark.unit
def test_binding_runs_hides_private_runner_runs_of_other_users(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    """Project standing must not reveal a run executing on someone else's
    private machine — mirrors the global runs list's gate."""
    from pi_dash.runner.models import DevMachine, Runner

    other = _make_member(workspace, project, email="owner@example.com", project_role=15)
    machine = DevMachine.objects.create(owner=other, host_label="theirs.local", label="Theirs")
    runner = Runner.objects.create(
        owner=other,
        workspace=workspace,
        pod=default_pod,
        name="their-private-runner",
        dev_machine=machine,
    )
    _make_run(binding, default_pod, other, runner=runner)
    visible = _make_run(binding, default_pod, other)  # runner-less: visible

    res = _get_binding_runs(workspace, project, binding, create_user)
    assert res.status_code == 200
    assert [r["id"] for r in res.data["results"]] == [str(visible.id)]

    # The runner's owner sees both.
    res = _get_binding_runs(workspace, project, binding, other)
    assert res.status_code == 200
    assert res.data["total_count"] == 2


# ---------------------------------------------------------------------------
# Binding detail serializer: resolved_prompt / run_count
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_binding_detail_exposes_resolved_prompt_and_run_count(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    binding.extra_context = "Focus on the API."
    binding.save(update_fields=["extra_context"])
    _make_run(binding, default_pod, create_user)

    factory = APIRequestFactory()
    request = factory.get(
        f"/api/workspaces/{workspace.slug}/projects/{project.id}"
        f"/scheduler-bindings/{binding.id}/"
    )
    force_authenticate(request, user=create_user)
    res = ProjectSchedulerBindingDetailEndpoint.as_view()(
        request,
        slug=workspace.slug,
        project_id=str(project.id),
        binding_id=str(binding.id),
    )
    assert res.status_code == 200
    # Composed exactly like a dispatched run: scheduler prompt, extra
    # context, then the outcome-mode directive.
    assert res.data["resolved_prompt"].startswith("Audit the project.\n\nFocus on the API.")
    assert len(res.data["resolved_prompt"]) > len("Audit the project.\n\nFocus on the API.")
    assert res.data["run_count"] == 1
    assert res.data["scheduler_source"] == scheduler.source
    assert res.data["scheduler_is_enabled"] is True


# ---------------------------------------------------------------------------
# Run detail view: project-member grant for scheduler runs
# ---------------------------------------------------------------------------


def _get_run_detail(run, user):
    factory = APIRequestFactory()
    request = factory.get(f"/api/runners/runs/{run.id}/")
    force_authenticate(request, user=user)
    return AgentRunDetailEndpoint.as_view()(request, run_id=str(run.id))


@pytest.mark.unit
def test_run_detail_visible_to_project_member_for_scheduler_run(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    member = _make_member(workspace, project, email="member@example.com", project_role=15)
    run = _make_run(binding, default_pod, create_user)

    res = _get_run_detail(run, member)
    assert res.status_code == 200
    assert res.data["scheduler_binding_detail"]["scheduler_slug"] == "nightly-audit"


@pytest.mark.unit
def test_run_detail_hidden_from_workspace_member_outside_project(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    outsider = _make_member(workspace, project, email="outsider2@example.com", project_role=None)
    run = _make_run(binding, default_pod, create_user)

    res = _get_run_detail(run, outsider)
    assert res.status_code == 404


# ---------------------------------------------------------------------------
# Global runs list: ?scheduler_binding= filter
# ---------------------------------------------------------------------------


def _get_runs_list(user, query):
    factory = APIRequestFactory()
    request = factory.get("/api/runners/runs/", query)
    force_authenticate(request, user=user)
    return AgentRunListEndpoint.as_view()(request)


@pytest.mark.unit
def test_global_runs_list_filters_by_scheduler_binding(
    db, workspace, project, scheduler, binding, default_pod, create_user
):
    other_binding = _make_second_binding(project, workspace, create_user)
    mine = _make_run(binding, default_pod, create_user)
    _make_run(other_binding, default_pod, create_user)

    res = _get_runs_list(create_user, {"scheduler_binding": str(binding.id)})
    assert res.status_code == 200
    assert [r["id"] for r in res.data["results"]] == [str(mine.id)]


@pytest.mark.unit
def test_global_runs_list_rejects_malformed_scheduler_binding(db, workspace, create_user):
    res = _get_runs_list(create_user, {"scheduler_binding": "not-a-uuid"})
    assert res.status_code == 400
