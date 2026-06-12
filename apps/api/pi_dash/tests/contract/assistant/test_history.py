# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""History replay truncation — only the newest N completed turns reach the model."""

import datetime

import pytest
from django.utils import timezone

from pi_dash.assistant.models import AssistantThread, AssistantTurn, TurnStatus
from pi_dash.assistant.runtime.history import load_history

pytestmark = pytest.mark.django_db


def test_load_history_replays_only_newest_turns(world, settings, mocker):
    settings.ASSISTANT_HISTORY_MAX_TURNS = 3
    # Pass stored blobs through verbatim so the test controls their shape.
    mocker.patch(
        "pydantic_ai.messages.ModelMessagesTypeAdapter.validate_python",
        side_effect=lambda blob: blob,
    )
    thread = AssistantThread.objects.create(workspace=world.ws, user=world.member)
    t0 = timezone.now() - datetime.timedelta(minutes=10)
    for i in range(5):
        turn = AssistantTurn.objects.create(
            thread=thread, status=TurnStatus.COMPLETED, model_messages=[f"turn-{i}"]
        )
        # auto_now_add would make all turns the same instant; space them out
        AssistantTurn.objects.filter(pk=turn.pk).update(
            created_at=t0 + datetime.timedelta(minutes=i)
        )

    # newest 3 turns, in chronological order
    assert load_history(thread) == ["turn-2", "turn-3", "turn-4"]
