# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The data step of ``runner/0044_delete_own_agent_chat_history`` (NEWPIDASH-332).

What can go wrong is the data, not the schema: the delete must remove every
chat session, message, event, approval and dedupe row on own-agent runners,
leave engine runners' chats untouched, and change nothing on a second run.
The test executes the migration's exact statements, so it fails if the SQL
drifts from what the migration ships.
"""

from __future__ import annotations

import importlib

import pytest
from django.db import connection
from django.utils import timezone

from pi_dash.runner.models import (
    AgentChatApprovalRequest,
    AgentChatEvent,
    AgentChatMessage,
    AgentChatMessageRole,
    AgentChatMessageStatus,
    AgentChatSession,
    ApprovalKind,
    ChatMessageDedupe,
    Pod,
    Runner,
    RunnerProvisioning,
    RunnerStatus,
)

migration = importlib.import_module("pi_dash.runner.migrations.0044_delete_own_agent_chat_history")


def apply_delete() -> None:
    with connection.cursor() as cursor:
        for statement in migration.DELETE_OWN_AGENT_CHAT:
            cursor.execute(statement)


def make_runner(create_user, workspace, pod, name, provisioning) -> Runner:
    now = timezone.now()
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=now,
        refresh_token_generation=1,
        enrolled_at=now,
        provisioning=provisioning,
    )


def make_tree(runner, workspace, create_user, pod, tag) -> AgentChatSession:
    session = AgentChatSession.objects.create(
        workspace=workspace,
        runner=runner,
        created_by=create_user,
        pod=pod,
    )
    first = AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.USER,
        content=f"hi {tag}",
        status=AgentChatMessageStatus.COMPLETED,
        seq=1,
    )
    AgentChatMessage.objects.create(
        session=session,
        role=AgentChatMessageRole.ASSISTANT,
        content=f"yo {tag}",
        status=AgentChatMessageStatus.COMPLETED,
        seq=2,
    )
    AgentChatEvent.objects.create(
        session=session,
        message=first,
        seq=1,
        kind="chat_turn",
        payload={"tag": tag},
    )
    AgentChatApprovalRequest.objects.create(
        session=session,
        local_approval_id=f"appr-{tag}",
        kind=ApprovalKind.COMMAND_EXECUTION,
        payload={},
    )
    ChatMessageDedupe.objects.create(session=session, message_id=f"mid-{tag}")
    return session


@pytest.fixture
def chat_runners(db, create_user, workspace, project):
    pod = Pod.default_for_project(project)
    own = make_runner(create_user, workspace, pod, "own-agent", RunnerProvisioning.MANUAL)
    engine = make_runner(create_user, workspace, pod, "engine", RunnerProvisioning.DESKTOP_BUNDLED)
    return own, engine


@pytest.mark.unit
def test_delete_removes_own_agent_chats_and_keeps_engine(db, create_user, workspace, project, chat_runners):
    own, engine = chat_runners
    make_tree(own, workspace, create_user, own.pod, "own")
    make_tree(engine, workspace, create_user, engine.pod, "eng")

    apply_delete()

    for model, expected_own, expected_engine in [
        (AgentChatSession, 0, 1),
        (AgentChatMessage, 0, 2),
        (AgentChatEvent, 0, 1),
        (AgentChatApprovalRequest, 0, 1),
        (ChatMessageDedupe, 0, 1),
    ]:
        assert (
            model.objects.filter(session__runner=own).count()
            if model is not AgentChatSession
            else model.objects.filter(runner=own).count()
        ) == expected_own, model.__name__
        assert (
            model.objects.filter(session__runner=engine).count()
            if model is not AgentChatSession
            else model.objects.filter(runner=engine).count()
        ) == expected_engine, model.__name__


@pytest.mark.unit
def test_delete_is_idempotent(db, create_user, workspace, project, chat_runners):
    own, _engine = chat_runners
    make_tree(own, workspace, create_user, own.pod, "own")

    apply_delete()
    apply_delete()

    assert AgentChatSession.objects.filter(runner=own).count() == 0
    assert AgentChatMessage.objects.filter(session__runner=own).count() == 0
    assert AgentChatEvent.objects.filter(session__runner=own).count() == 0
    assert AgentChatApprovalRequest.objects.filter(session__runner=own).count() == 0
    assert ChatMessageDedupe.objects.filter(session__runner=own).count() == 0
