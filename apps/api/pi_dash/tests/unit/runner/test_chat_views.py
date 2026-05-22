# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import uuid
from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.db.models import User, WorkspaceMember
from pi_dash.runner.models import (
    AgentChatApprovalRequest,
    AgentChatEvent,
    AgentChatMessage,
    AgentChatMessageRole,
    AgentChatMessageStatus,
    AgentChatSession,
    AgentChatSessionStatus,
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import tokens


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="chat-runner",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


@pytest.fixture
def runner_token(enrolled_runner):
    token = tokens.mint_access_token(
        runner_id=str(enrolled_runner.id),
        user_id=str(enrolled_runner.owner_id),
        workspace_id=str(enrolled_runner.workspace_id),
        rtg=1,
    )
    return token.raw


@pytest.mark.unit
def test_runner_chat_failed_closes_close_requested_session(
    db, api_client, create_user, workspace, pod, enrolled_runner, runner_token
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        active_turn_id="turn_1",
        close_requested=True,
    )
    message = AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.USER,
        content="stop after this",
        status=AgentChatMessageStatus.SENT,
        seq=1,
    )
    session.active_message_id = message.id
    session.save(update_fields=["active_message_id", "updated_at"])

    resp = api_client.post(
        f"/api/v1/runner/chat/sessions/{session.id}/failed/",
        {"code": "agent_failed", "detail": "boom"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=uuid.uuid4().hex,
    )

    assert resp.status_code == 200, resp.data
    session.refresh_from_db()
    message.refresh_from_db()
    assert session.status == AgentChatSessionStatus.CLOSED
    assert session.closed_at is not None
    assert session.active_message_id is None
    assert session.active_turn_id == ""
    assert message.status == AgentChatMessageStatus.FAILED
    assert AgentChatEvent.objects.filter(session=session, kind="chat_failed").exists()
    assert AgentChatEvent.objects.filter(session=session, kind="chat_closed").exists()


@pytest.mark.unit
def test_runner_chat_delta_string_is_persisted_and_completed(
    db, api_client, create_user, workspace, pod, enrolled_runner, runner_token
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        active_turn_id="turn_1",
    )
    message = AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.USER,
        content="hello",
        status=AgentChatMessageStatus.SENT,
        seq=1,
    )
    session.active_message_id = message.id
    session.save(update_fields=["active_message_id", "updated_at"])

    event = api_client.post(
        f"/api/v1/runner/chat/sessions/{session.id}/events/",
        {
            "kind": "assistant_delta",
            "bridge_seq": 1,
            "payload": {
                "method": "item/agentMessage/delta",
                "params": {"delta": "Hello"},
            },
        },
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=uuid.uuid4().hex,
    )
    assert event.status_code == 200, event.data
    assistant = AgentChatMessage.objects.get(
        session=session,
        role=AgentChatMessageRole.ASSISTANT,
    )
    assert assistant.content == "Hello"
    assert assistant.status == AgentChatMessageStatus.STREAMING

    complete = api_client.post(
        f"/api/v1/runner/chat/sessions/{session.id}/messages/{message.id}/complete/",
        {"status": "completed"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
        HTTP_IDEMPOTENCY_KEY=uuid.uuid4().hex,
    )
    assert complete.status_code == 200, complete.data
    assistant.refresh_from_db()
    message.refresh_from_db()
    session.refresh_from_db()
    assert assistant.content == "Hello"
    assert assistant.status == AgentChatMessageStatus.COMPLETED
    assert message.status == AgentChatMessageStatus.COMPLETED
    assert session.active_message_id is None
    assert session.active_turn_id == ""


@pytest.mark.unit
def test_chat_completion_drains_queued_task_after_releasing_runner(
    db, api_client, create_user, workspace, pod, enrolled_runner, runner_token
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        active_turn_id="turn_1",
    )
    message = AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.USER,
        content="hello",
        status=AgentChatMessageStatus.SENT,
        seq=1,
    )
    session.active_message_id = message.id
    session.save(update_fields=["active_message_id", "updated_at"])
    queued = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        prompt="queued while chat is active",
        status=AgentRunStatus.QUEUED,
    )

    with (
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
        patch("pi_dash.runner.services.pubsub.send_to_runner") as send_to_runner,
    ):
        resp = api_client.post(
            f"/api/v1/runner/chat/sessions/{session.id}/messages/{message.id}/complete/",
            {"status": "completed"},
            format="json",
            HTTP_AUTHORIZATION=f"Bearer {runner_token}",
            HTTP_IDEMPOTENCY_KEY=uuid.uuid4().hex,
        )

    assert resp.status_code == 200, resp.data
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.runner_id == enrolled_runner.id
    send_to_runner.assert_called_once()
    assert send_to_runner.call_args.args[1]["type"] == "assign"


@pytest.mark.unit
def test_chat_failure_drains_queued_task_after_releasing_runner(
    db, api_client, create_user, workspace, pod, enrolled_runner, runner_token
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        active_turn_id="turn_1",
    )
    message = AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.USER,
        content="hello",
        status=AgentChatMessageStatus.SENT,
        seq=1,
    )
    session.active_message_id = message.id
    session.save(update_fields=["active_message_id", "updated_at"])
    queued = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        prompt="queued while chat is active",
        status=AgentRunStatus.QUEUED,
    )

    with (
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
        patch("pi_dash.runner.services.pubsub.send_to_runner") as send_to_runner,
    ):
        resp = api_client.post(
            f"/api/v1/runner/chat/sessions/{session.id}/failed/",
            {"code": "agent_failed", "detail": "boom"},
            format="json",
            HTTP_AUTHORIZATION=f"Bearer {runner_token}",
            HTTP_IDEMPOTENCY_KEY=uuid.uuid4().hex,
        )

    assert resp.status_code == 200, resp.data
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.runner_id == enrolled_runner.id
    send_to_runner.assert_called_once()
    assert send_to_runner.call_args.args[1]["type"] == "assign"


@pytest.mark.unit
def test_chat_message_get_is_not_send_throttled(db, session_client, create_user, workspace, pod, enrolled_runner):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
    )
    with patch("pi_dash.runner.views.chat.ChatSendThrottle.allow_request", return_value=False):
        resp = session_client.get(f"/api/runners/chat/sessions/{session.id}/messages/")
    assert resp.status_code == 200


@pytest.mark.unit
def test_second_chat_message_is_rejected_while_first_dispatch_is_active(
    db, session_client, create_user, workspace, pod, enrolled_runner
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
    )
    with (
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
        patch("pi_dash.runner.services.chat.send_to_runner") as send_to_runner,
    ):
        first = session_client.post(
            f"/api/runners/chat/sessions/{session.id}/messages/",
            {"content": "first"},
            format="json",
        )
        second = session_client.post(
            f"/api/runners/chat/sessions/{session.id}/messages/",
            {"content": "second"},
            format="json",
        )

    assert first.status_code == 201, first.data
    assert second.status_code == 409
    assert second.data["error"] == "chat_turn_active"
    assert send_to_runner.call_count == 1


@pytest.mark.unit
def test_chat_warm_dispatches_without_creating_message(
    db, session_client, create_user, workspace, pod, enrolled_runner
):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        local_session_id="claude-session-1",
        local_thread_id="claude-session-1",
    )
    with (
        patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()),
        patch("pi_dash.runner.services.chat.send_to_runner") as send_to_runner,
    ):
        resp = session_client.post(f"/api/runners/chat/sessions/{session.id}/warm/")

    assert resp.status_code == 202, resp.data
    assert AgentChatMessage.objects.filter(session=session).count() == 0
    send_to_runner.assert_called_once()
    _runner_id, payload = send_to_runner.call_args.args
    assert payload["type"] == "chat_warm"
    assert payload["chat_session_id"] == str(session.id)
    assert payload["local_session_id"] == "claude-session-1"


@pytest.mark.unit
def test_chat_warm_skips_active_turn_without_dispatch(db, session_client, create_user, workspace, pod, enrolled_runner):
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=create_user,
        pod=pod,
        active_turn_id="turn_1",
    )
    with patch("pi_dash.runner.services.chat.send_to_runner") as send_to_runner:
        resp = session_client.post(f"/api/runners/chat/sessions/{session.id}/warm/")

    assert resp.status_code == 200, resp.data
    assert resp.data["skipped"] == "chat_turn_active"
    send_to_runner.assert_not_called()


@pytest.mark.unit
def test_workspace_admin_lists_pending_chat_approvals_for_other_users(
    db, session_client, create_user, workspace, pod, enrolled_runner
):
    other = User.objects.create(
        email=f"other-{uuid.uuid4().hex[:8]}@example.com",
        username=f"other_{uuid.uuid4().hex[:8]}",
    )
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=enrolled_runner,
        created_by=other,
        pod=pod,
    )
    approval = AgentChatApprovalRequest.objects.create(
        session=session,
        local_approval_id="approval_1",
        kind="command_execution",
        payload={"command": "echo ok"},
    )

    resp = session_client.get("/api/runners/chat/approvals/", {"workspace": str(workspace.id)})

    assert resp.status_code == 200
    assert [row["id"] for row in resp.data] == [str(approval.id)]
