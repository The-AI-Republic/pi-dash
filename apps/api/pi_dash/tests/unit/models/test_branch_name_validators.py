# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.core.exceptions import ValidationError

from pi_dash.db.models import Issue, Project


# These tests target the regex validator attached to
# ``Project.base_branch`` / ``Issue.git_work_branch`` only. Running
# ``full_clean`` on the whole model would also enforce every other
# required field (``updated_by``, ``icon_prop``, ``default_state``, …),
# turning a focused validator test into a fixture-completeness test.
# Instead we ask each field for its own validators and run them — the
# regex behaviour is what's actually under test here.


def _validate_field(model_cls, field_name, value):
    """Run the validators registered on ``model_cls.<field_name>`` against
    ``value``. Equivalent to what ``full_clean`` would do for that field
    in isolation; raises ``ValidationError`` on rejection."""
    model_cls._meta.get_field(field_name).run_validators(value)


@pytest.mark.unit
@pytest.mark.parametrize(
    "branch",
    [
        "",
        "main",
        "develop",
        "feat/pinned",
        "release/1.2.3",
        "user/jdoe/fix_42",
    ],
)
def test_project_base_branch_accepts_valid_names(branch):
    _validate_field(Project, "base_branch", branch)


@pytest.mark.unit
@pytest.mark.parametrize(
    "branch",
    [
        "feat branch",          # space
        "feat\tname",           # tab
        "branch;rm -rf /",      # shell metacharacter
        "ref@{1}",              # git reflog syntax, but disallowed charset here
        "feat*",                # glob
        "branch~1",             # ancestor ref
    ],
)
def test_project_base_branch_rejects_invalid_names(branch):
    with pytest.raises(ValidationError):
        _validate_field(Project, "base_branch", branch)


@pytest.mark.unit
@pytest.mark.parametrize(
    "branch",
    [
        "",
        "feat/pinned",
        "hotfix/oom_crash",
    ],
)
def test_issue_git_work_branch_accepts_valid_names(branch):
    _validate_field(Issue, "git_work_branch", branch)


# Note: the server-side regex is intentionally a subset of git's own
# `check-ref-format`; it mirrors the client-side form regex. It does not
# reject leading-dash names (e.g. "-rf") because `-` is a valid branch
# character. The runner's `validate_branch_name` is the load-bearing
# guard against `git` flag smuggling.
@pytest.mark.unit
@pytest.mark.parametrize(
    "branch",
    [
        "feat branch",      # space
        "feat\nname",       # newline
        "ref@{1}",          # `@` and braces are outside the charset
        "feat branch?",     # `?`
    ],
)
def test_issue_git_work_branch_rejects_invalid_names(branch):
    with pytest.raises(ValidationError):
        _validate_field(Issue, "git_work_branch", branch)
