# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Save-time override validation (design §6.3) and sample-context sync."""

from __future__ import annotations

import pytest

from pi_dash.prompting import recipes
from pi_dash.prompting.context import build_context, build_scheduler_context
from pi_dash.prompting.validation import (
    MAX_BODY_LENGTH,
    OverrideValidationError,
    kinds_for_section,
    sample_contexts,
    validate_override,
)


@pytest.mark.unit
def test_validate_override_accepts_clean_body(db, workspace):
    # A plain-text override with no Jinja is always valid.
    validate_override("implementation", "Just do the work, carefully.", workspace=workspace)


@pytest.mark.unit
def test_validate_override_accepts_valid_jinja_referencing_known_vars(db, workspace):
    validate_override(
        "implementation",
        "Implement {{ issue.identifier }} on branch {{ repo.work_branch }}.",
        workspace=workspace,
    )


@pytest.mark.unit
def test_validate_override_rejects_syntax_error(db, workspace):
    with pytest.raises(OverrideValidationError):
        validate_override("implementation", "{% if x %}unclosed", workspace=workspace)


@pytest.mark.unit
def test_validate_override_rejects_unknown_variable(db, workspace):
    with pytest.raises(OverrideValidationError):
        validate_override(
            "implementation", "{{ nonexistent_var }}", workspace=workspace
        )


@pytest.mark.unit
def test_validate_override_rejects_locked_section(db, workspace):
    with pytest.raises(OverrideValidationError):
        validate_override("pidash-cli", "anything", workspace=workspace)


@pytest.mark.unit
def test_validate_override_rejects_oversized_body(db, workspace):
    with pytest.raises(OverrideValidationError):
        validate_override(
            "implementation", "x" * (MAX_BODY_LENGTH + 1), workspace=workspace
        )


@pytest.mark.unit
def test_validate_override_minimal_context_trap(db, workspace):
    # Referencing parent.title without a guard renders fine in the populated
    # sample but crashes in the minimal one (parent=None) → rejected.
    with pytest.raises(OverrideValidationError):
        validate_override(
            "analyze-and-scope", "Parent is {{ parent.title }}.", workspace=workspace
        )


@pytest.mark.unit
def test_kinds_for_section_shared_vs_unique():
    # implementation is coding-task only; session-framing is in all three.
    assert kinds_for_section("implementation") == ["coding-task"]
    assert set(kinds_for_section("session-framing")) == {
        "coding-task",
        "review",
        "scheduler",
    }


# ----------------------------------------------------------------------
# Sample contexts must stay in sync with the real builders (design §6.3)
# ----------------------------------------------------------------------


def _issue_context_keys(db, workspace, create_user):
    from pi_dash.db.models import Issue, Project, State
    from pi_dash.runner.models import AgentRun

    project = Project.objects.create(
        name="Ctx Project", identifier="CTX", workspace=workspace, created_by=create_user
    )
    state = State.objects.create(name="In Progress", project=project, group="started")
    issue = Issue.objects.create(
        name="Ctx issue",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        workspace=workspace, prompt="", work_item=issue, created_by=create_user
    )
    return set(build_context(issue, run).keys())


@pytest.mark.unit
def test_issue_sample_context_keys_match_builder(db, workspace, create_user):
    builder_keys = _issue_context_keys(db, workspace, create_user)
    for ctx in sample_contexts("coding-task"):
        assert set(ctx.keys()) == builder_keys


@pytest.mark.unit
def test_scheduler_sample_context_keys_match_builder(db, workspace, create_user):
    from pi_dash.db.models.project import Project
    from pi_dash.db.models.scheduler import Scheduler, SchedulerBinding
    from pi_dash.runner.models import AgentRun
    from django.utils import timezone

    project = Project.objects.filter(workspace=workspace).first()
    scheduler = Scheduler.objects.create(
        workspace=workspace, slug="s", name="S", prompt="do x"
    )
    binding = SchedulerBinding.objects.create(
        scheduler=scheduler,
        project=project,
        workspace=workspace,
        dtstart=timezone.now(),
    )
    run = AgentRun.objects.create(workspace=workspace, prompt="", created_by=create_user)
    builder_keys = set(build_scheduler_context(binding, run).keys())
    for ctx in sample_contexts(recipes.KIND_SCHEDULER):
        assert set(ctx.keys()) == builder_keys
