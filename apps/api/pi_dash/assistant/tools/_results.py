# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tool-return helpers: untrusted-content delimiting and write-activity rows.

Only the model-facing dict is returned to the agent loop; the human-facing
transcript row (with links) is persisted separately so write actions are always
visible in the chat. See ``.ai_design/integrate_ai_agent/02-backend.md`` §4.3.
"""

from __future__ import annotations

from typing import Any, Optional

from pi_dash.assistant.models import AssistantThread, AssistantTurn, MessageKind
from pi_dash.assistant.runtime import events


def wrap_untrusted(text: Optional[str]) -> str:
    """Wrap user-generated text so the model treats it as data, not instructions.

    The closing delimiter is neutralized inside the content so it cannot be
    forged by a malicious issue/comment body.
    """
    safe = (text or "").replace("</untrusted>", "<​/untrusted>")
    return f"<untrusted>{safe}</untrusted>"


def truncate(text: Optional[str], limit: int) -> tuple[str, bool]:
    s = text or ""
    if len(s) <= limit:
        return s, False
    return s[:limit], True


def issue_link(deps, issue) -> dict[str, Any]:
    return {
        "type": "issue",
        "workspace_slug": deps.workspace_slug,
        "project_id": str(issue.project_id),
        "issue_id": str(issue.id),
        "url_path": f"/{deps.workspace_slug}/projects/{issue.project_id}/issues/{issue.id}",
    }


def record_write(deps, summary: str, links: Optional[list[dict[str, Any]]] = None) -> None:
    """Persist a tool-activity row + event so the user sees what changed."""
    thread = AssistantThread.objects.get(pk=deps.thread_id)
    turn = AssistantTurn.objects.filter(pk=deps.turn_id).first()
    message = events.create_message(
        thread,
        MessageKind.TOOL_RESULT,
        turn=turn,
        display_content=summary,
        payload={"links": links or []},
    )
    events.append_event(
        thread,
        "tool_result",
        payload={
            "turn_id": str(deps.turn_id),
            "message": events.message_envelope(message),
        },
        message=message,
        turn=turn,
    )
