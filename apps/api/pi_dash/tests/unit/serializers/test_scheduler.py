# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the project-scheduler serializers.

Covers cron validation, extra_context bounds, the scheduler-rebind lock
on PATCH, and the active_binding_count fallback.
"""

from __future__ import annotations

import pytest
from crum import impersonate

from pi_dash.app.serializers.scheduler import (
    EXTRA_CONTEXT_MAX_LENGTH,
    SchedulerBindingSerializer,
    SchedulerSerializer,
)
from pi_dash.db.models import Project, Scheduler, SchedulerBinding


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )


@pytest.fixture
def scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="security-audit",
            name="Security Audit",
            prompt="Scan the project.",
        )


@pytest.fixture
def other_scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="gdpr",
            name="GDPR",
            prompt="Check GDPR compliance.",
        )


@pytest.fixture
def binding(scheduler, project, workspace, create_user):
    with impersonate(create_user):
        return SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            cron="* * * * *",
            actor=create_user,
        )


# ---------------------------------------------------------------------------
# Cron validation
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "cron",
    [
        "* * * * *",
        "0 * * * *",
        "*/5 * * * *",
        "0 0 * * 0",
    ],
)
def test_valid_cron_accepted(scheduler, project, workspace, cron):
    s = SchedulerBindingSerializer(
        data={"scheduler": scheduler.id, "project": project.id, "cron": cron}
    )
    assert s.is_valid(), s.errors


@pytest.mark.unit
@pytest.mark.parametrize(
    "cron",
    [
        "",
        "not a cron",
        "* * * *",  # 4 fields
        "* * * * * *",  # 6 fields — explicitly disallowed (Codex #8)
        "60 * * * *",  # invalid minute
    ],
)
def test_invalid_cron_rejected(scheduler, project, workspace, cron):
    s = SchedulerBindingSerializer(
        data={"scheduler": scheduler.id, "project": project.id, "cron": cron}
    )
    assert not s.is_valid()
    assert "cron" in s.errors


# ---------------------------------------------------------------------------
# extra_context bounds  (Codex review #5)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_extra_context_at_max_length_accepted(scheduler, project, workspace):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "cron": "* * * * *",
            "extra_context": "x" * EXTRA_CONTEXT_MAX_LENGTH,
        }
    )
    assert s.is_valid(), s.errors


@pytest.mark.unit
def test_extra_context_over_max_length_rejected(scheduler, project, workspace):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "cron": "* * * * *",
            "extra_context": "x" * (EXTRA_CONTEXT_MAX_LENGTH + 1),
        }
    )
    assert not s.is_valid()
    assert "extra_context" in s.errors


# ---------------------------------------------------------------------------
# PATCH cannot repoint scheduler / project  (Codex review #1)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_patch_cannot_change_scheduler(binding, other_scheduler):
    s = SchedulerBindingSerializer(
        binding,
        data={"scheduler": other_scheduler.id},
        partial=True,
    )
    assert not s.is_valid()
    assert "scheduler" in s.errors


@pytest.mark.unit
def test_patch_cannot_change_project(binding, workspace, create_user):
    with impersonate(create_user):
        other_project = Project.objects.create(
            name="API",
            identifier="API",
            workspace=workspace,
            created_by=create_user,
        )
    s = SchedulerBindingSerializer(
        binding,
        data={"project": other_project.id},
        partial=True,
    )
    assert not s.is_valid()
    assert "project" in s.errors


@pytest.mark.unit
def test_patch_can_change_cron_and_enabled(binding):
    s = SchedulerBindingSerializer(
        binding,
        data={"cron": "0 */6 * * *", "enabled": False},
        partial=True,
    )
    assert s.is_valid(), s.errors


# ---------------------------------------------------------------------------
# active_binding_count fallback  (Codex review #12)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_active_binding_count_uses_annotation_when_present(scheduler):
    scheduler._active_binding_count = 5
    data = SchedulerSerializer(scheduler).data
    assert data["active_binding_count"] == 5


@pytest.mark.unit
def test_active_binding_count_falls_back_to_count_query(scheduler, binding):
    # No annotation set; serializer should query bindings.count()
    fresh = Scheduler.objects.get(pk=scheduler.pk)
    data = SchedulerSerializer(fresh).data
    assert data["active_binding_count"] == 1


@pytest.mark.unit
def test_active_binding_count_excludes_soft_deleted_bindings(scheduler, binding):
    binding.delete()  # soft-delete
    fresh = Scheduler.objects.get(pk=scheduler.pk)
    data = SchedulerSerializer(fresh).data
    assert data["active_binding_count"] == 0
