# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import types

import pytest
from pydantic_ai import ModelRetry

from pi_dash.assistant.models import (
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
)
from pi_dash.assistant.tools import _scoping, comments, issues, runs
from pi_dash.db.models import Issue, IssueComment
from pi_dash.tests.contract.assistant.conftest import (
    ROLE_ADMIN,
    ROLE_GUEST,
    ROLE_MEMBER,
    fake_ctx,
    make_deps,
)

pytestmark = pytest.mark.django_db


@pytest.fixture
def member_ctx(world):
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    turn = AssistantTurn.objects.create(thread=thread)
    deps = make_deps(world.member, world.ws, ROLE_MEMBER, thread_id=thread.id, turn_id=turn.id)
    return fake_ctx(deps), thread, turn


@pytest.fixture
def admin_ctx(world):
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.admin)
    turn = AssistantTurn.objects.create(thread=thread)
    deps = make_deps(world.admin, world.ws, ROLE_ADMIN, thread_id=thread.id, turn_id=turn.id)
    return fake_ctx(deps), thread, turn


@pytest.fixture
def guest_ctx(world):
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.guest)
    turn = AssistantTurn.objects.create(thread=thread)
    deps = make_deps(world.guest, world.ws, ROLE_GUEST, thread_id=thread.id, turn_id=turn.id)
    return fake_ctx(deps), thread, turn


def test_list_projects_scoped(world, member_ctx):
    from pi_dash.assistant.tools import projects

    ctx, *_ = member_ctx
    result = projects.list_projects(ctx)
    names = {p["name"] for p in result}
    assert names == {"Alpha"}  # member is only in project A


def test_create_issue_sets_attribution_and_sequence(world, member_ctx):
    ctx, thread, turn = member_ctx
    res = issues.create_issue(ctx, project_id=str(world.proj_a.id), name="New bug", description_md="Steps")
    assert res["created"] is True

    issue = Issue.objects.get(id=res["id"])
    assert issue.created_via == "assistant"
    assert issue.created_by_id == world.member.id
    assert issue.sequence_id == 3  # issue_a=1, guest_issue=2 -> next is 3
    assert issue.state_id == world.todo.id  # default state
    assert "Steps" in (issue.description_html or "")

    # a tool-activity row was written so the user sees the action
    assert AssistantMessage.objects.filter(thread=thread, kind=MessageKind.TOOL_RESULT).exists()


def test_create_issue_guest_denied(world, guest_ctx):
    ctx, *_ = guest_ctx
    with pytest.raises(_scoping.ToolPermissionError):
        issues.create_issue(ctx, project_id=str(world.proj_a.id), name="nope")


def test_create_comment_attribution(world, member_ctx):
    ctx, *_ = member_ctx
    res = comments.create_comment(ctx, issue_id=str(world.issue_a.id), body_md="Looking into it")
    assert res["created"] is True
    comment = IssueComment.objects.get(id=res["comment_id"])
    assert comment.speaker_type == "agent"
    assert comment.speaker_label == "Pi Dash AI"
    assert comment.actor_id == world.member.id


def test_guest_comment_only_on_own_issue(world, guest_ctx):
    ctx, *_ = guest_ctx
    # guest may comment on their own issue
    res = comments.create_comment(ctx, issue_id=str(world.guest_issue.id), body_md="more info")
    assert res["created"] is True
    # but not on an issue they didn't create
    with pytest.raises(_scoping.ToolPermissionError):
        comments.create_comment(ctx, issue_id=str(world.issue_a.id), body_md="hi")


def test_update_issue_fields(world, member_ctx):
    ctx, *_ = member_ctx
    res = issues.update_issue(ctx, issue_id=str(world.issue_a.id), name="Renamed", priority="high")
    assert res["updated"] is True
    assert set(res["changed"]) == {"name", "priority"}
    world.issue_a.refresh_from_db()
    assert world.issue_a.name == "Renamed"
    assert world.issue_a.priority == "high"


def test_create_issue_with_parent(world, member_ctx):
    ctx, *_ = member_ctx
    res = issues.create_issue(
        ctx,
        project_id=str(world.proj_a.id),
        name="Sub-task",
        parent_issue_id=str(world.issue_a.id),
    )
    assert res["created"] is True
    assert res["parent_id"] == str(world.issue_a.id)
    issue = Issue.objects.get(id=res["id"])
    assert issue.parent_id == world.issue_a.id


def test_create_issue_parent_must_be_same_project(world, admin_ctx):
    ctx, *_ = admin_ctx
    # admin can access both projects, but issue_b (proj_b) still can't parent a
    # proj_a child — the same-project guard, not the access guard, rejects it.
    with pytest.raises(ModelRetry) as exc:
        issues.create_issue(
            ctx,
            project_id=str(world.proj_a.id),
            name="Sub-task",
            parent_issue_id=str(world.issue_b.id),
        )
    assert "same project" in str(exc.value)


def test_create_issue_parent_not_accessible(world, member_ctx):
    ctx, *_ = member_ctx
    # member is not in proj_b, so issue_b is not accessible as a parent.
    with pytest.raises(_scoping.ToolNotFound):
        issues.create_issue(
            ctx,
            project_id=str(world.proj_a.id),
            name="Sub-task",
            parent_issue_id=str(world.issue_b.id),
        )


def test_update_issue_set_and_unlink_parent(world, member_ctx):
    ctx, *_ = member_ctx
    # set parent
    res = issues.update_issue(
        ctx, issue_id=str(world.guest_issue.id), parent_issue_id=str(world.issue_a.id)
    )
    assert res["updated"] is True
    assert "parent" in res["changed"]
    world.guest_issue.refresh_from_db()
    assert world.guest_issue.parent_id == world.issue_a.id

    # unlink via null
    res = issues.update_issue(ctx, issue_id=str(world.guest_issue.id), parent_issue_id=None)
    assert "parent" in res["changed"]
    world.guest_issue.refresh_from_db()
    assert world.guest_issue.parent_id is None


def test_update_issue_omitted_parent_left_unchanged(world, member_ctx):
    ctx, *_ = member_ctx
    world.guest_issue.parent = world.issue_a
    world.guest_issue.save(update_fields=["parent"])
    # update another field without passing parent_issue_id
    res = issues.update_issue(ctx, issue_id=str(world.guest_issue.id), name="Renamed")
    assert "parent" not in res["changed"]
    world.guest_issue.refresh_from_db()
    assert world.guest_issue.parent_id == world.issue_a.id  # untouched


def test_update_issue_reject_self_parent(world, member_ctx):
    ctx, *_ = member_ctx
    with pytest.raises(ModelRetry):
        issues.update_issue(
            ctx, issue_id=str(world.issue_a.id), parent_issue_id=str(world.issue_a.id)
        )


def test_update_issue_reject_cycle(world, member_ctx):
    ctx, *_ = member_ctx
    # guest_issue -> issue_a (child of issue_a)
    world.guest_issue.parent = world.issue_a
    world.guest_issue.save(update_fields=["parent"])
    # now try to make issue_a a child of guest_issue -> cycle
    with pytest.raises(ModelRetry):
        issues.update_issue(
            ctx, issue_id=str(world.issue_a.id), parent_issue_id=str(world.guest_issue.id)
        )


def test_dispatch_coding_run(world, member_ctx, mocker):
    ctx, *_ = member_ctx
    fake_run = types.SimpleNamespace(id="11111111-1111-1111-1111-111111111111")
    outcome = types.SimpleNamespace(created_run=fake_run, reason="ok")
    mocker.patch(
        "pi_dash.orchestration.service.handle_issue_state_transition", return_value=outcome
    )
    res = runs.dispatch_coding_run(ctx, issue_id=str(world.issue_a.id))
    assert res["dispatched"] is True
    assert res["run_id"] == str(fake_run.id)
    assert res["new_state"] == "In Progress"
