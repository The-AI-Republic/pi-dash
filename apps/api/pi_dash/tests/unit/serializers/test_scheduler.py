# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the project-scheduler serializers.

Covers RRULE/dtstart validation, color validation, extra_context bounds,
the scheduler-rebind lock on PATCH, and the active_binding_count fallback.
"""

from __future__ import annotations

from datetime import datetime, timezone as dt_timezone

import pytest
from crum import impersonate

from pi_dash.app.serializers.scheduler import (
    EXTRA_CONTEXT_MAX_LENGTH,
    SchedulerBindingSerializer,
    SchedulerSerializer,
)
from pi_dash.db.models import Project, Scheduler, SchedulerBinding


# Fixture-shared anchor — Mon 2026-01-05 09:00 UTC.
_ANCHOR = datetime(2026, 1, 5, 9, 0, tzinfo=dt_timezone.utc)


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
    # Use a slug that does NOT collide with the builtins seeded by the
    # ``post_save(Workspace)`` signal in ``pi_dash.scheduler.signals`` (e.g.
    # ``security-audit``). The constraint
    # ``scheduler_unique_workspace_slug_when_active`` would otherwise reject
    # this insert because the seeded builtin already occupies that slot.
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="test-scheduler",
            name="Test Scheduler",
            prompt="Scan the project.",
        )


@pytest.fixture
def other_scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="test-other-scheduler",
            name="Test Other Scheduler",
            prompt="Check GDPR compliance.",
        )


@pytest.fixture
def binding(scheduler, project, workspace, create_user):
    with impersonate(create_user):
        return SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            dtstart=_ANCHOR,
            rrule="FREQ=MINUTELY",
            tzid="UTC",
            actor=create_user,
        )


# ---------------------------------------------------------------------------
# RRULE validation
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "rrule",
    [
        "FREQ=MINUTELY",
        "FREQ=HOURLY;BYMINUTE=15",
        "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        "FREQ=WEEKLY;BYDAY=MO,WE,FR",
        "",  # empty = single-shot at dtstart; allowed
    ],
)
def test_valid_rrule_accepted(scheduler, project, workspace, rrule):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "dtstart": _ANCHOR.isoformat(),
            "rrule": rrule,
            "tzid": "UTC",
        }
    )
    assert s.is_valid(), s.errors


@pytest.mark.unit
@pytest.mark.parametrize(
    "rrule",
    [
        "FREQ=SECONDLY",  # rejected — DoS vector against Beat tick
        "not a real rrule",
    ],
)
def test_invalid_rrule_rejected(scheduler, project, workspace, rrule):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "dtstart": _ANCHOR.isoformat(),
            "rrule": rrule,
            "tzid": "UTC",
        }
    )
    assert not s.is_valid()
    assert "rrule" in s.errors


@pytest.mark.unit
def test_invalid_tzid_rejected(scheduler, project, workspace):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "dtstart": _ANCHOR.isoformat(),
            "rrule": "FREQ=DAILY",
            "tzid": "Mars/Olympus_Mons",
        }
    )
    assert not s.is_valid()
    assert "tzid" in s.errors


@pytest.mark.unit
def test_rdates_must_be_iso_strings(scheduler, project, workspace):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "dtstart": _ANCHOR.isoformat(),
            "rrule": "FREQ=DAILY",
            "rdates": ["not-a-datetime"],
        }
    )
    assert not s.is_valid()
    assert "rdates" in s.errors


# ---------------------------------------------------------------------------
# Color validation (Scheduler)
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize("color", ["#3b82f6", "#000000", "#FFFFFF"])
def test_valid_color_accepted(workspace, color):
    s = SchedulerSerializer(
        data={"slug": "x", "name": "x", "prompt": "x", "color": color}
    )
    assert s.is_valid(), s.errors


@pytest.mark.unit
@pytest.mark.parametrize("color", ["", "blue", "#fff", "#ggghhh", "rgb(0,0,0)"])
def test_invalid_color_rejected(workspace, color):
    s = SchedulerSerializer(
        data={"slug": "x", "name": "x", "prompt": "x", "color": color}
    )
    assert not s.is_valid()
    assert "color" in s.errors


# ---------------------------------------------------------------------------
# extra_context bounds  (Codex review #5)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_extra_context_at_max_length_accepted(scheduler, project, workspace):
    s = SchedulerBindingSerializer(
        data={
            "scheduler": scheduler.id,
            "project": project.id,
            "dtstart": _ANCHOR.isoformat(),
            "rrule": "FREQ=MINUTELY",
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
            "dtstart": _ANCHOR.isoformat(),
            "rrule": "FREQ=MINUTELY",
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
def test_patch_can_change_rrule_and_enabled(binding):
    s = SchedulerBindingSerializer(
        binding,
        data={"rrule": "FREQ=HOURLY;INTERVAL=6", "enabled": False},
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
