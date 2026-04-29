# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.bgtasks.scheduler``.

Covers the three-phase claim/dispatch/rollback flow in
``fire_scheduler_binding`` plus the scanner's filter semantics.
"""

from __future__ import annotations

from datetime import timedelta
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.bgtasks.scheduler import (
    fire_scheduler_binding,
    scan_due_bindings,
)
from pi_dash.db.models import Project, Scheduler, SchedulerBinding
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


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
def runner_for_workspace(db, workspace, create_user):
    pod = Pod.default_for_workspace(workspace)
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentA",
        credential_hash="h",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def stub_drain(monkeypatch):
    """Force `transaction.on_commit` to fire its callback synchronously
    and stub `drain_pod_by_id` so tests don't try to assign work to a
    real runner."""
    from pi_dash.runner.services import matcher

    drain_mock = mock.Mock()
    monkeypatch.setattr(matcher, "drain_pod_by_id", drain_mock)
    monkeypatch.setattr(
        "django.db.transaction.on_commit",
        lambda fn, **kw: fn(),
    )
    return drain_mock


@pytest.fixture
def scheduler(workspace, create_user):
    with impersonate(create_user):
        return Scheduler.objects.create(
            workspace=workspace,
            slug="security-audit",
            name="Security Audit",
            description="Test",
            prompt="Scan the project.",
        )


@pytest.fixture
def binding(scheduler, project, workspace, create_user, runner_for_workspace):
    """A binding due immediately (next_run_at = NULL) with a valid cron."""
    with impersonate(create_user):
        return SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            cron="* * * * *",
            extra_context="",
            enabled=True,
            actor=create_user,
        )


# ---------------------------------------------------------------------------
# scan_due_bindings
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_scan_picks_up_due_bindings(binding):
    # binding has next_run_at=None → due
    count = scan_due_bindings.__wrapped__()
    assert count == 1


@pytest.mark.unit
def test_scan_skips_disabled_bindings(binding):
    binding.enabled = False
    binding.save(update_fields=["enabled"])
    count = scan_due_bindings.__wrapped__()
    assert count == 0


@pytest.mark.unit
def test_scan_skips_bindings_whose_scheduler_is_disabled(binding, scheduler):
    scheduler.is_enabled = False
    scheduler.save(update_fields=["is_enabled"])
    count = scan_due_bindings.__wrapped__()
    assert count == 0


@pytest.mark.unit
def test_scan_skips_soft_deleted_scheduler(binding, scheduler):
    """Codex review #4: soft-deleted scheduler's bindings must not fire
    even if cascade soft-delete hasn't run yet."""
    # Soft-delete the scheduler directly (skip the cascade so the binding
    # row still has deleted_at IS NULL, simulating the async-cascade window).
    Scheduler.all_objects.filter(pk=scheduler.pk).update(deleted_at=timezone.now())
    count = scan_due_bindings.__wrapped__()
    assert count == 0


@pytest.mark.unit
def test_scan_respects_future_next_run_at(binding):
    binding.next_run_at = timezone.now() + timedelta(hours=1)
    binding.save(update_fields=["next_run_at"])
    count = scan_due_bindings.__wrapped__()
    assert count == 0


# ---------------------------------------------------------------------------
# fire_scheduler_binding — happy path
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fire_creates_run_and_advances_next_run_at(binding):
    fired = fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    assert fired is True
    binding.refresh_from_db()
    assert binding.last_run_id is not None
    assert binding.last_run.status == AgentRunStatus.QUEUED
    assert binding.last_run.scheduler_binding_id == binding.pk
    assert binding.last_run.work_item_id is None
    assert binding.next_run_at is not None
    assert binding.next_run_at > timezone.now()
    assert binding.last_error == ""


@pytest.mark.unit
def test_fire_resolves_prompt_with_extra_context(scheduler, binding):
    binding.extra_context = "Focus on authn paths."
    binding.save(update_fields=["extra_context"])
    fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    binding.refresh_from_db()
    run = binding.last_run
    assert "Scan the project." in run.prompt
    assert "Focus on authn paths." in run.prompt


@pytest.mark.unit
def test_fire_phase3a_save_advances_updated_at(binding):
    """Codex review #6: Phase 3a must use save() so auto_now fires."""
    before = binding.updated_at
    fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    binding.refresh_from_db()
    assert binding.updated_at > before


# ---------------------------------------------------------------------------
# fire_scheduler_binding — skip / rollback paths
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_fire_skips_when_last_run_in_flight(binding, workspace, create_user):
    """Concurrency policy: a non-terminal previous run blocks the next tick."""
    pod = Pod.default_for_workspace(workspace)
    in_flight = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        scheduler_binding=binding,
        status=AgentRunStatus.RUNNING,
        prompt="prior tick",
    )
    binding.last_run = in_flight
    prior_next_run_at = timezone.now() - timedelta(seconds=5)
    binding.next_run_at = prior_next_run_at
    binding.save(update_fields=["last_run", "next_run_at"])

    fired = fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    assert fired is False
    binding.refresh_from_db()
    # next_run_at must NOT have advanced — skip is silent
    assert binding.next_run_at == prior_next_run_at


@pytest.mark.unit
def test_fire_disables_binding_with_bad_cron_and_clears_next_run_at(binding):
    """Codex review #10: bad-cron disable must also clear next_run_at."""
    binding.cron = "this is not a cron"
    binding.next_run_at = timezone.now() - timedelta(seconds=5)
    binding.save(update_fields=["cron", "next_run_at"])

    fired = fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    assert fired is False
    binding.refresh_from_db()
    assert binding.enabled is False
    assert binding.next_run_at is None
    assert "invalid cron" in binding.last_error


@pytest.mark.unit
def test_fire_rolls_back_next_run_at_on_dispatch_failure(monkeypatch, binding):
    """Phase 3b: when dispatch returns None, restore prior next_run_at."""
    binding.next_run_at = None  # NULL — true "first run, due now"
    binding.save(update_fields=["next_run_at"])

    monkeypatch.setattr(
        "pi_dash.bgtasks.scheduler.dispatch_scheduler_run",
        lambda b, p: None,
    )
    fired = fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    assert fired is False
    binding.refresh_from_db()
    # Rolled back to NULL — no scheduled time advanced since dispatch failed
    assert binding.next_run_at is None
    assert "dispatch failed" in binding.last_error


@pytest.mark.unit
def test_fire_returns_false_when_binding_deleted(binding):
    binding.delete()  # soft-delete
    fired = fire_scheduler_binding.__wrapped__(fire_scheduler_binding, str(binding.pk))
    assert fired is False
