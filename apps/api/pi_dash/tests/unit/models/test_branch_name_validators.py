# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.core.exceptions import ValidationError

from pi_dash.db.models import Issue, Project, State


@pytest.fixture
def project(workspace, create_user):
    return Project.objects.create(
        name="Branch Validators",
        identifier="BV",
        workspace=workspace,
        created_by=create_user,
    )


@pytest.fixture
def issue(workspace, project, create_user):
    state = State.objects.create(name="Todo", project=project, group="unstarted")
    return Issue.objects.create(
        name="Task",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )


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
def test_project_base_branch_accepts_valid_names(project, branch):
    project.base_branch = branch
    project.full_clean()


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
def test_project_base_branch_rejects_invalid_names(project, branch):
    project.base_branch = branch
    with pytest.raises(ValidationError):
        project.full_clean()


@pytest.mark.unit
@pytest.mark.parametrize(
    "branch",
    [
        "",
        "feat/pinned",
        "hotfix/oom_crash",
    ],
)
def test_issue_git_work_branch_accepts_valid_names(issue, branch):
    issue.git_work_branch = branch
    issue.full_clean()


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
def test_issue_git_work_branch_rejects_invalid_names(issue, branch):
    issue.git_work_branch = branch
    with pytest.raises(ValidationError):
        issue.full_clean()
