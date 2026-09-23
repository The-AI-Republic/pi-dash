# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant relation tools (PDASHOSS01-199): scoping + idempotency."""

from unittest import mock

import pytest
from pydantic_ai import ModelRetry

from pi_dash.assistant.models import AssistantMessage, AssistantThread, AssistantTurn, MessageKind
from pi_dash.assistant.tools import _scoping, issues
from pi_dash.db.models import IssueRelation
from pi_dash.tests.contract.assistant.conftest import (
    ROLE_ADMIN,
    ROLE_GUEST,
    ROLE_MEMBER,
    _issue,
    fake_ctx,
    make_deps,
)

pytestmark = pytest.mark.django_db


@pytest.fixture(autouse=True)
def _no_activity():
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


def _ctx(world, user, role):
    thread = AssistantThread.objects.create(workspace=world.ws, user=user)
    turn = AssistantTurn.objects.create(thread=thread)
    return fake_ctx(make_deps(user, world.ws, role, thread_id=thread.id, turn_id=turn.id)), thread


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


@pytest.fixture
def model_issue(world):
    return _issue(world.proj_a, world.admin, "Model", seq=3, state=world.todo)


def test_relate_list_unrelate_round_trip(world, model_issue):
    ctx, thread = _ctx(world, world.member, ROLE_MEMBER)
    handler = world.issue_a

    res = issues.relate_issues(
        ctx, issue=_ident(handler), relation_type="blocked_by", related_issues=[_ident(model_issue)]
    )
    assert res["created"] == [_ident(model_issue)]
    assert res["unchanged"] == [] and res["conflicts"] == []
    item = res["relations"]["blocked_by"][0]
    assert item["identifier"] == _ident(model_issue)
    assert item["name"] == "<untrusted>Model</untrusted>"
    assert item["state"] == "Todo"
    assert AssistantMessage.objects.filter(thread=thread, kind=MessageKind.TOOL_RESULT).count() == 1

    # Idempotent re-relate (by UUID this time): unchanged, no second activity row.
    again = issues.relate_issues(
        ctx, issue=str(handler.id), relation_type="blocked_by", related_issues=[str(model_issue.id)]
    )
    assert again["created"] == [] and again["unchanged"] == [_ident(model_issue)]
    assert AssistantMessage.objects.filter(thread=thread, kind=MessageKind.TOOL_RESULT).count() == 1

    listed = issues.list_issue_relations(ctx, issue=_ident(model_issue))
    assert [i["identifier"] for i in listed["relations"]["blocking"]] == [_ident(handler)]

    removed = issues.unrelate_issues(
        ctx, issue=_ident(handler), relation_type="blocked_by", related_issues=[_ident(model_issue)]
    )
    assert removed["removed"] == [_ident(model_issue)]
    assert removed["relations"]["blocked_by"] == []
    assert not IssueRelation.objects.exists()


def test_cannot_relate_to_an_issue_in_a_project_the_user_cannot_see(world):
    # The member is not in project B; its issue must be indistinguishable from
    # a missing one, and nothing is written.
    ctx, _ = _ctx(world, world.member, ROLE_MEMBER)
    with pytest.raises(_scoping.ToolNotFound):
        issues.relate_issues(
            ctx, issue=_ident(world.issue_a), relation_type="blocked_by", related_issues=[_ident(world.issue_b)]
        )
    with pytest.raises(_scoping.ToolNotFound):
        issues.relate_issues(
            ctx, issue=str(world.issue_b.id), relation_type="blocked_by", related_issues=[_ident(world.issue_a)]
        )
    assert not IssueRelation.objects.exists()


def test_cross_workspace_reference_is_not_found(world):
    from pi_dash.tests.contract.assistant.conftest import _project

    other_project = _project(world.other_ws, world.other_user, "Other", "OTH")
    foreign = _issue(other_project, world.other_user, "Foreign", seq=1)
    ctx, _ = _ctx(world, world.admin, ROLE_ADMIN)
    with pytest.raises(_scoping.ToolNotFound):
        issues.relate_issues(
            ctx, issue=_ident(world.issue_a), relation_type="blocked_by", related_issues=[str(foreign.id)]
        )
    assert not IssueRelation.objects.exists()


def test_admin_can_relate_across_projects_they_belong_to(world):
    ctx, _ = _ctx(world, world.admin, ROLE_ADMIN)
    res = issues.relate_issues(
        ctx, issue=_ident(world.issue_a), relation_type="blocked_by", related_issues=[_ident(world.issue_b)]
    )
    assert res["created"] == [_ident(world.issue_b)]

    # The member (not in project B) sees the edge's source issue but not the
    # hidden end.
    member_ctx, _ = _ctx(world, world.member, ROLE_MEMBER)
    assert issues.list_issue_relations(member_ctx, issue=_ident(world.issue_a))["relations"]["blocked_by"] == []


def test_guest_cannot_relate(world, model_issue):
    ctx, _ = _ctx(world, world.guest, ROLE_GUEST)
    with pytest.raises(_scoping.ToolPermissionError):
        issues.relate_issues(
            ctx, issue=_ident(world.guest_issue), relation_type="blocked_by", related_issues=[_ident(model_issue)]
        )
    # Reading is fine.
    assert issues.list_issue_relations(ctx, issue=_ident(world.guest_issue))["issue"] == _ident(world.guest_issue)


@pytest.mark.parametrize(
    "relation_type,related",
    [("depends_on", "model"), ("blocked_by", "self"), ("blocked_by", "none")],
)
def test_invalid_requests_are_retries(world, model_issue, relation_type, related):
    ctx, _ = _ctx(world, world.member, ROLE_MEMBER)
    refs = {"model": [_ident(model_issue)], "self": [_ident(world.issue_a)], "none": []}[related]
    with pytest.raises(ModelRetry):
        issues.relate_issues(ctx, issue=_ident(world.issue_a), relation_type=relation_type, related_issues=refs)
    assert not IssueRelation.objects.exists()
