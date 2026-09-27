# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Matcher-level eligibility for *issue-backed* runs in a shared pod.

``filter_runs_usable_by_runner`` accepts a run for a PRIVATE runner when
``runner.owner`` is the run's ``created_by``/``owner``, the issue's
``created_by``, or one of ``issue.assignees``. ``test_drain_pod`` covers
only the free-form (no ``work_item``) case; this module pins the
issue-backed clauses — in particular that assignment is what lets a
second member's runner take work they did not create.

Scenario throughout: one project, one default pod, two members (Alice
and Luke) who each registered a private runner into that pod.
"""

from __future__ import annotations

from unittest.mock import patch
from uuid import uuid4

import pytest
from crum import impersonate
from django.db import transaction
from django.utils import timezone

from pi_dash.db.models import Issue, State, User, WorkspaceMember
from pi_dash.db.models.issue import IssueAssignee
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def alice(create_user):
    """Workspace owner from the conftest fixture; the issue creator."""
    return create_user


@pytest.fixture
def luke(db, workspace):
    suffix = uuid4().hex[:6]
    user = User.objects.create(
        email=f"luke-{suffix}@example.com",
        username=f"luke_{suffix}",
        first_name="Luke",
    )
    user.set_password("pw")
    user.save()
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=15)
    return user


def _runner(owner, workspace, pod, name):
    return Runner.objects.create(
        owner=owner,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def alice_runner(alice, workspace, pod):
    return _runner(alice, workspace, pod, "alice-mbp")


@pytest.fixture
def luke_runner(luke, workspace, pod):
    return _runner(luke, workspace, pod, "luke-desktop")


@pytest.fixture
def backlog(project, create_user):
    with impersonate(create_user):
        return State.objects.create(
            name="Backlog", project=project, group="backlog", default=True, sequence=100
        )


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
        yield mock


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


def _issue(workspace, project, backlog, creator, assignees=()):
    """Create an issue in Backlog (non-ticking, so no dispatch auto-fires)."""
    with impersonate(creator):
        issue = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=backlog,
            created_by=creator,
        )
    for user in assignees:
        IssueAssignee.objects.create(issue=issue, assignee=user, workspace=workspace, project=project)
    return issue


def _queued_run(workspace, pod, issue):
    """Mirror the orchestration dispatch: created_by = issue.created_by,
    owner unset until a runner is assigned."""
    return AgentRun.objects.create(
        workspace=workspace,
        created_by=issue.created_by,
        owner=None,
        pod=pod,
        work_item=issue,
        status=AgentRunStatus.QUEUED,
        prompt="do the thing",
    )


def _visible_to(runner, run):
    """True if the matcher would hand ``run`` to ``runner``."""
    with transaction.atomic():
        picked = matcher.next_for_runner(runner)
    return picked is not None and picked.pk == run.pk


# ---------------------------------------------------------------------------
# Row 1: created by Alice, assigned to Luke -> both runners eligible
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_assignee_runner_is_eligible_for_issue_it_did_not_create(
    db, workspace, project, pod, backlog, alice, luke, alice_runner, luke_runner
):
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[luke])
    run = _queued_run(workspace, pod, issue)

    assert _visible_to(alice_runner, run) is True, "creator's runner must be eligible"
    assert _visible_to(luke_runner, run) is True, "assignee's runner must be eligible"


@pytest.mark.unit
def test_assigned_issue_is_actually_assigned_by_drain_pod(
    db, workspace, project, pod, backlog, alice, luke, luke_runner
):
    """Luke's runner alone in the pod: assignment alone must be enough."""
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[luke])
    run = _queued_run(workspace, pod, issue)

    assert matcher.drain_pod(pod) == 1
    run.refresh_from_db()
    assert run.runner_id == luke_runner.id
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.owner_id == luke.id, "billing follows the machine, not created_by"


# ---------------------------------------------------------------------------
# Row 2: created by Alice, unassigned -> only Alice's runner
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_unassigned_issue_is_invisible_to_other_members_runner(
    db, workspace, project, pod, backlog, alice, luke, alice_runner, luke_runner
):
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[])
    run = _queued_run(workspace, pod, issue)

    assert _visible_to(alice_runner, run) is True
    assert _visible_to(luke_runner, run) is False, "co-member is not entitled without assignment"


@pytest.mark.unit
def test_drain_pod_leaves_unassigned_issue_queued_for_other_members_runner(
    db, workspace, project, pod, backlog, alice, luke, luke_runner
):
    """Only Luke's runner is in the pod. The run must stay QUEUED, not leak."""
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[])
    run = _queued_run(workspace, pod, issue)

    assert matcher.drain_pod(pod) == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


# ---------------------------------------------------------------------------
# Row 3: created by Luke, assigned to Luke -> only Luke's runner
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_other_members_issue_is_invisible_to_alices_runner(
    db, workspace, project, pod, backlog, alice, luke, alice_runner, luke_runner
):
    issue = _issue(workspace, project, backlog, creator=luke, assignees=[luke])
    run = _queued_run(workspace, pod, issue)

    assert _visible_to(luke_runner, run) is True
    assert _visible_to(alice_runner, run) is False, (
        "workspace admin standing must not let Alice's machine take Luke's work"
    )


@pytest.mark.unit
def test_drain_pod_does_not_give_lukes_issue_to_alices_runner(
    db, workspace, project, pod, backlog, alice, luke, alice_runner
):
    issue = _issue(workspace, project, backlog, creator=luke, assignees=[luke])
    run = _queued_run(workspace, pod, issue)

    assert matcher.drain_pod(pod) == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


# ---------------------------------------------------------------------------
# Un-assignment withdraws the grant
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_removing_assignee_revokes_eligibility_while_still_queued(
    db, workspace, project, pod, backlog, alice, luke, luke_runner
):
    """Un-assignment must take Luke's runner back out of the eligible set.

    ``IssueAssignee`` is a ``SoftDeleteModel`` and the un-assign path only
    stamps ``deleted_at``; the eligibility reads therefore have to go through
    the through-model's live manager, not the ``Issue.assignees`` M2M (whose
    join ignores ``deleted_at`` and would grandfather every past assignee).
    """
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[luke])
    run = _queued_run(workspace, pod, issue)
    assert _visible_to(luke_runner, run) is True

    # Exactly what the issue serializer does when assignees are edited.
    IssueAssignee.objects.filter(issue=issue).delete()
    assert IssueAssignee.all_objects.filter(issue=issue, assignee=luke).exists(), (
        "precondition: the row is soft-deleted, not removed"
    )

    assert _visible_to(luke_runner, run) is False
    assert matcher.drain_pod(pod) == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None


@pytest.mark.unit
def test_preflight_ignores_soft_deleted_assignee(
    db, workspace, project, pod, backlog, alice, luke, luke_runner
):
    """The creation-time preflight must agree with the dispatch gate."""
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[luke])
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, alice.id) is True

    IssueAssignee.objects.filter(issue=issue).delete()

    assert matcher.pod_has_runner_for_issue_principal(pod, issue, alice.id) is False


@pytest.mark.unit
def test_reassignment_after_removal_restores_eligibility(
    db, workspace, project, pod, backlog, alice, luke, luke_runner
):
    """Re-assigning leaves a soft-deleted row behind a live one; the live
    row must still be found (and must not be double-counted)."""
    issue = _issue(workspace, project, backlog, creator=alice, assignees=[luke])
    run = _queued_run(workspace, pod, issue)
    IssueAssignee.objects.filter(issue=issue).delete()
    IssueAssignee.objects.create(issue=issue, assignee=luke, workspace=workspace, project=project)

    assert IssueAssignee.all_objects.filter(issue=issue, assignee=luke).count() == 2
    assert _visible_to(luke_runner, run) is True
    assert matcher.drain_pod(pod) == 1
