# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fire + dispatch — happy path queues a turn, turn_active skips, rotation."""

from __future__ import annotations

import datetime

import pytest
from django.utils import timezone

from pi_dash.assistant.errors import MAX_THREAD_MESSAGES
from pi_dash.assistant.models import (
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
    ThreadKind,
    TurnStatus,
)
from pi_dash.bgtasks import loop as loop_tasks
from pi_dash.db.models import SkipReason
from pi_dash.tests.contract.assistant.conftest import configure_llm
from pi_dash.tests.contract.loop.conftest import make_job, make_target

pytestmark = pytest.mark.django_db


def test_fire_happy_path_queues_turn(world, kms_crypto, mocker, django_capture_on_commit_callbacks):
    delay = mocker.patch("pi_dash.loop.dispatch.run_assistant_turn.delay")
    configure_llm(world.member)
    job = make_job(prompt="close merged PR issues")
    target = make_target(job, world.ws, world.member)

    with django_capture_on_commit_callbacks(execute=True):
        assert loop_tasks.fire_loop_target(str(target.id)) is True

    target.refresh_from_db()
    # A hidden loop thread was created and carries the active turn.
    thread = target.thread
    assert thread is not None
    assert thread.kind == ThreadKind.LOOP
    assert thread.active_turn_id is not None
    # The user message is the job prompt verbatim.
    msg = AssistantMessage.objects.get(thread=thread, kind=MessageKind.USER)
    assert msg.display_content == "close merged PR issues"
    assert target.last_run_id == thread.active_turn_id
    delay.assert_called_once()


def test_fire_skips_when_turn_active(world, kms_crypto, mocker):
    mocker.patch("pi_dash.loop.dispatch.run_assistant_turn.delay")
    configure_llm(world.member)
    job = make_job()
    # Pre-create a loop thread with an in-flight turn.
    thread = AssistantThread.objects.create(
        workspace=world.ws, user=world.member, kind=ThreadKind.LOOP
    )
    turn = AssistantTurn.objects.create(thread=thread, status=TurnStatus.RUNNING)
    thread.active_turn = turn
    thread.save(update_fields=["active_turn"])
    target = make_target(job, world.ws, world.member, thread=thread)

    assert loop_tasks.fire_loop_target(str(target.id)) is False
    target.refresh_from_db()
    assert target.last_skip_reason == SkipReason.TURN_ACTIVE
    # No second turn was created.
    assert AssistantTurn.objects.filter(thread=thread).count() == 1


def test_fire_noop_when_cursor_in_future(world, kms_crypto):
    configure_llm(world.member)
    job = make_job()
    future = timezone.now() + datetime.timedelta(hours=1)
    target = make_target(job, world.ws, world.member, next_run_at=future)
    assert loop_tasks.fire_loop_target(str(target.id)) is False


def test_dispatch_rotates_full_thread(world, kms_crypto, mocker, django_capture_on_commit_callbacks):
    mocker.patch("pi_dash.loop.dispatch.run_assistant_turn.delay")
    settings_headroom = 30
    configure_llm(world.member)
    job = make_job()
    old_thread = AssistantThread.objects.create(
        workspace=world.ws, user=world.member, kind=ThreadKind.LOOP
    )
    # Fill the thread past the rotation threshold.
    full_count = MAX_THREAD_MESSAGES - settings_headroom + 1
    AssistantMessage.objects.bulk_create(
        [
            AssistantMessage(thread=old_thread, seq=i, kind=MessageKind.ASSISTANT, display_content="x")
            for i in range(full_count)
        ]
    )
    target = make_target(job, world.ws, world.member, thread=old_thread)

    with django_capture_on_commit_callbacks(execute=True):
        loop_tasks.fire_loop_target(str(target.id))

    target.refresh_from_db()
    assert target.thread_id != old_thread.id  # rotated to a fresh thread
    old_thread.refresh_from_db()
    assert old_thread.is_archived is True
