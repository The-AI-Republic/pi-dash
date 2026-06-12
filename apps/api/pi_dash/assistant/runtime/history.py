# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""LLM history (de)serialization.

The ONLY replay source is ``AssistantTurn.model_messages`` — the verbatim
serialized pydantic-ai message list captured per completed turn. Concatenating
completed turns yields a valid, correctly-alternating history by construction.
See ``.ai_design/integrate_ai_agent/02-backend.md`` §1.
"""

from __future__ import annotations

import logging
from typing import Any

from pi_dash.assistant.models import AssistantThread, TurnStatus

logger = logging.getLogger(__name__)


def load_history(thread: AssistantThread) -> list:
    """Return the prior ``list[ModelMessage]`` for this thread (completed turns)."""
    from pydantic_ai.messages import ModelMessagesTypeAdapter

    messages: list = []
    blobs = (
        thread.turns.filter(status=TurnStatus.COMPLETED, model_messages__isnull=False)
        .order_by("created_at", "id")  # stable tie-break for same-microsecond turns
        .values_list("model_messages", flat=True)
    )
    for blob in blobs:
        if not blob:
            continue
        try:
            messages.extend(ModelMessagesTypeAdapter.validate_python(blob))
        except Exception:  # noqa: BLE001 — a pydantic-ai format change shouldn't crash a turn
            logger.warning("assistant: could not deserialize a stored turn's history; skipping it")
            continue
    return messages


def dump_new_messages(result) -> list[Any]:
    """Serialize ``result.new_messages()`` to a JSON-able list for storage."""
    from pydantic_ai.messages import ModelMessagesTypeAdapter

    return ModelMessagesTypeAdapter.dump_python(result.new_messages(), mode="json")
