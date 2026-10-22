# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The web app's scheduler create / install / delete routes after
PDASHOSS01-318 moved their bodies into ``pi_dash.scheduler`` helpers shared
with the ``/api/v1/`` surface. Behaviour is unchanged."""

from __future__ import annotations

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework.test import APIRequestFactory, force_authenticate

from pi_dash.app.views.scheduler.views import (
    ProjectSchedulerBindingListEndpoint,
    WorkspaceSchedulerDetailEndpoint,
    WorkspaceSchedulerListEndpoint,
)
from pi_dash.db.models import Project, ProjectMember, Scheduler, SchedulerBinding


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        p = Project.objects.create(
            name="Web", identifier="WEB", workspace=workspace, created_by=create_user
        )
    ProjectMember.objects.get_or_create(
        project=p, member=create_user, defaults={"role": 20, "is_active": True}
    )
    return p


def _call(view, method, url, user, data=None, **kwargs):
    factory = APIRequestFactory()
    request = getattr(factory, method)(url, data, format="json")
    force_authenticate(request, user=user)
    return view.as_view()(request, **kwargs)


@pytest.mark.unit
def test_create_install_delete(db, workspace, project, create_user):
    created = _call(
        WorkspaceSchedulerListEndpoint,
        "post",
        "/schedulers/",
        create_user,
        {"name": "Scan", "slug": "scan", "prompt": "Scan it."},
        slug=workspace.slug,
    )
    assert created.status_code == 201
    scheduler_id = created.data["id"]

    duplicate = _call(
        WorkspaceSchedulerListEndpoint,
        "post",
        "/schedulers/",
        create_user,
        {"name": "Scan", "slug": "scan", "prompt": "Scan it."},
        slug=workspace.slug,
    )
    assert duplicate.status_code == 400
    assert "slug" in duplicate.data

    installed = _call(
        ProjectSchedulerBindingListEndpoint,
        "post",
        "/scheduler-bindings/",
        create_user,
        {
            "scheduler": scheduler_id,
            "dtstart": "2026-01-01T09:00:00Z",
            "rrule": "FREQ=DAILY",
        },
        slug=workspace.slug,
        project_id=str(project.id),
    )
    assert installed.status_code == 201
    binding = SchedulerBinding.objects.get(pk=installed.data["id"])
    assert binding.project_id == project.id
    assert binding.actor_id == create_user.id
    assert binding.next_run_at > timezone.now()

    deleted = _call(
        WorkspaceSchedulerDetailEndpoint,
        "delete",
        "/schedulers/x/",
        create_user,
        slug=workspace.slug,
        scheduler_id=scheduler_id,
    )
    assert deleted.status_code == 204
    assert not Scheduler.objects.filter(pk=scheduler_id).exists()
    assert SchedulerBinding.all_objects.get(pk=binding.pk).deleted_at is not None
