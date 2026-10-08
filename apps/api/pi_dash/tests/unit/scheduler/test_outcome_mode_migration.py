# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The data step of ``db/0169_remove_schedulerbinding_outcome_mode``.

The schema step is Django's; what can go wrong is the data: a binding must
dispatch the same task after the migration as it did before it, when the
platform still appended its outcome mode's directive.
"""

from __future__ import annotations

import importlib
from types import SimpleNamespace

import pytest

from pi_dash.prompting.context import build_scheduler_task_body
from pi_dash.scheduler.builtins import BUILTINS

migration = importlib.import_module("pi_dash.db.migrations.0169_remove_schedulerbinding_outcome_mode")

PROMPT = "Scan the project for bugs."


def _body_before(mode: str, prompt: str, extra_context: str) -> str:
    """The pre-migration ``build_scheduler_task_body``."""
    parts = [prompt.strip(), extra_context.strip(), migration.DIRECTIVES[mode]]
    return "\n\n".join(p for p in parts if p)


def _body_after(mode: str, prompt: str, extra_context: str) -> str:
    binding = SimpleNamespace(
        scheduler=SimpleNamespace(prompt=prompt),
        extra_context=migration.migrated_extra_context(mode, prompt, extra_context),
    )
    return build_scheduler_task_body(binding)


@pytest.mark.unit
@pytest.mark.parametrize("mode", ["apply_fix", "fix_and_review"])
@pytest.mark.parametrize("extra_context", ["", "Focus on the auth module.", "  Trailing space.\n\n"])
@pytest.mark.parametrize("prompt", [PROMPT, BUILTINS[0].prompt, BUILTINS[1].prompt])
def test_fix_modes_render_the_same_task_body(mode, extra_context, prompt):
    assert _body_after(mode, prompt, extra_context) == _body_before(mode, prompt, extra_context)


@pytest.mark.unit
@pytest.mark.parametrize("extra_context", ["", "Focus on the auth module."])
def test_create_issue_renders_the_same_task_body_when_the_prompt_does_not_file(extra_context):
    assert _body_after("create_issue", PROMPT, extra_context) == _body_before("create_issue", PROMPT, extra_context)


@pytest.mark.unit
@pytest.mark.parametrize("builtin", BUILTINS, ids=lambda b: b.slug)
def test_create_issue_is_left_alone_when_the_prompt_already_files(builtin):
    """Both builtins carry their own filing command, so repeating the
    directive would only duplicate it."""
    assert migration.prompt_files_issues(builtin.prompt)
    assert migration.migrated_extra_context("create_issue", builtin.prompt, "") == ""
    assert migration.migrated_extra_context("create_issue", builtin.prompt, "Keep it short.") == "Keep it short."
    assert _body_after("create_issue", builtin.prompt, "") == builtin.prompt.strip()


@pytest.mark.unit
def test_unknown_mode_gets_the_create_issue_directive():
    """``outcome_mode_directive`` fell back to create-issue for a stale value."""
    assert _body_after("nonexistent", PROMPT, "") == _body_before("create_issue", PROMPT, "")


@pytest.mark.unit
@pytest.mark.parametrize("mode", ["create_issue", "apply_fix", "fix_and_review"])
@pytest.mark.parametrize("extra_context", ["", "Focus on the auth module."])
def test_reverse_recovers_mode_and_extra_context(mode, extra_context):
    folded = migration.migrated_extra_context(mode, PROMPT, extra_context)
    assert migration.split_extra_context(folded) == (mode, extra_context)


@pytest.mark.unit
def test_reverse_leaves_hand_written_extra_context_alone():
    assert migration.split_extra_context("File at most three issues.") == ("create_issue", "File at most three issues.")
