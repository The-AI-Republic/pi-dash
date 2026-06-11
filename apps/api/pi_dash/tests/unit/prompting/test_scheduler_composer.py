# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Scheduler runs composed through the unified composer (design §5)."""

from __future__ import annotations

import pytest
from django.utils import timezone

from pi_dash.db.models.project import Project
from pi_dash.db.models.scheduler import OutcomeMode, Scheduler, SchedulerBinding
from pi_dash.prompting.composer import build_scheduler_turn
from pi_dash.prompting.context import build_scheduler_context, build_scheduler_task_body
from pi_dash.runner.models import AgentRun


@pytest.fixture
def binding(db, workspace, create_user):
    project = Project.objects.filter(workspace=workspace).first()
    scheduler = Scheduler.objects.create(
        workspace=workspace,
        slug="nightly-audit",
        name="Nightly Audit",
        description="Scan the repo for issues.",
        prompt="Audit the codebase for security problems.",
    )
    return SchedulerBinding.objects.create(
        scheduler=scheduler,
        project=project,
        workspace=workspace,
        dtstart=timezone.now(),
        extra_context="Focus on the auth module.",
        outcome_mode=OutcomeMode.CREATE_ISSUE,
        actor=create_user,
    )


@pytest.fixture
def fake_run(db, workspace, create_user):
    return AgentRun.objects.create(workspace=workspace, prompt="", created_by=create_user)


@pytest.mark.unit
def test_scheduler_context_shape(binding, fake_run):
    ctx = build_scheduler_context(binding, fake_run)
    assert ctx["run"]["kind"] == "scheduler"
    assert ctx["scheduler"]["name"] == "Nightly Audit"
    assert ctx["project"]["identifier"]
    assert "issue" not in ctx  # no issue-centric keys


@pytest.mark.unit
def test_task_body_concatenates_prompt_extra_and_outcome(binding):
    body = build_scheduler_task_body(binding)
    assert "Audit the codebase for security problems." in body
    assert "Focus on the auth module." in body
    # outcome-mode directive appended (create-issue mode)
    assert "Work mode" in body


@pytest.mark.unit
def test_scheduler_turn_renders_and_injects_task_body(binding, fake_run):
    prompt = build_scheduler_turn(binding, fake_run)
    assert "{%" not in prompt and "{{" not in prompt
    assert "Audit the codebase for security problems." in prompt
    assert "Nightly Audit" in prompt
    # scheduler env, not issue env
    assert "PIDASH_PROJECT" in prompt
    assert "the current issue identifier" not in prompt
    # manifest stamped onto the run
    assert fake_run.prompt_manifest
    assert {e["section_key"] for e in fake_run.prompt_manifest} >= {
        "scheduler-task",
        "pidash-cli",
    }


@pytest.mark.unit
def test_operator_prompt_jinja_is_not_parsed(binding, fake_run):
    # The key §5.1 guarantee: operator-authored prompt text is injected as a
    # context variable, never parsed as Jinja. Literal braces survive verbatim.
    binding.scheduler.prompt = "Check {{ this }} and {% that %} literally."
    binding.scheduler.save(update_fields=["prompt"])
    prompt = build_scheduler_turn(binding, fake_run)
    assert "{{ this }}" in prompt
    assert "{% that %}" in prompt
