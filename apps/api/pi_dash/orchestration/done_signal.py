# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Parser for the agent's terminal ``pi-dash-done`` fenced block.

The runner forwards the agent's final message verbatim. The cloud-side parser
here extracts the fenced JSON, validates the minimal schema, and returns a
normalized payload suitable for persistence on ``AgentRun.done_payload``.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any, Dict, Optional

from django.utils import timezone


#: The closing fence is anchored to the start of a line so a stray triple
#: backtick inside a JSON string value (``summary``/``reason``/``notes`` are
#: all free-form) can't terminate the match early. In JSON, real newlines
#: inside string values must be escaped as ``\n``, so ``^```$`` is only reached
#: at a true fence boundary.
FENCE_RE = re.compile(
    r"^```pi-dash-done[ \t]*\n(?P<body>.*?)^```[ \t]*$",
    re.DOTALL | re.MULTILINE,
)

VALID_STATUSES = {"completed", "blocked", "noop"}


class DoneSignalError(ValueError):
    """Raised when the done signal is missing or malformed."""


@dataclass
class DoneSignal:
    status: str
    payload: Dict[str, Any]


def extract_fence(text: str) -> Optional[str]:
    if not text:
        return None
    # Prefer the last fence — the agent's contract emits it once, but any
    # earlier reference inside a code-of-code-block would stop here too.
    matches = list(FENCE_RE.finditer(text))
    if not matches:
        return None
    return matches[-1].group("body").strip()


def parse(text: str) -> DoneSignal:
    """Parse the terminal-turn output into a normalized payload."""
    body = extract_fence(text)
    if body is None:
        raise DoneSignalError("no pi-dash-done fenced block found")
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as exc:
        raise DoneSignalError(f"pi-dash-done JSON invalid: {exc.msg}") from exc

    if not isinstance(payload, dict):
        raise DoneSignalError("pi-dash-done payload must be a JSON object")

    status = payload.get("status")
    if status not in VALID_STATUSES:
        raise DoneSignalError(
            f"pi-dash-done.status must be one of {sorted(VALID_STATUSES)}; got {status!r}"
        )

    return DoneSignal(status=status, payload=_normalize(payload))


def _normalize(payload: Dict[str, Any]) -> Dict[str, Any]:
    """Fill in defaults for optional fields so downstream code doesn't have
    to guard every lookup."""
    autonomy = payload.get("autonomy") or {}
    autonomy = {
        "score": autonomy.get("score", 0),
        "type": autonomy.get("type", "none"),
        "reason": autonomy.get("reason", ""),
        "question_for_human": autonomy.get("question_for_human"),
        "safe_to_continue": autonomy.get("safe_to_continue", True),
    }

    state_transition = payload.get("state_transition") or {}
    state_transition = {
        "requested_group": state_transition.get("requested_group"),
        "reason": state_transition.get("reason"),
    }

    changes = payload.get("changes") or {}
    changes = {
        "branch": changes.get("branch"),
        "commits": changes.get("commits") or [],
        "files_touched": changes.get("files_touched") or [],
        "pr_url": changes.get("pr_url"),
    }

    validation = payload.get("validation") or {}
    validation = {
        "acceptance_all_met": validation.get("acceptance_all_met"),
        "ran": validation.get("ran") or [],
        "notes": validation.get("notes"),
    }

    progress = payload.get("progress") or {}
    progress = {
        "phase": progress.get("phase"),
        "checkpoints": progress.get("checkpoints") or {},
    }

    return {
        "status": payload["status"],
        "summary": payload.get("summary", ""),
        "state_transition": state_transition,
        "changes": changes,
        "validation": validation,
        "progress": progress,
        "autonomy": autonomy,
        "blockers": payload.get("blockers") or [],
    }


def ingest_into_run(run, text: str):
    """Parse ``text`` and persist the normalized payload onto ``run``.

    On malformed or missing fences the run is marked FAILED with the error;
    on success the payload is written to ``run.done_payload``, any prior
    ``error`` is cleared, and ``ended_at`` is stamped so the row is a complete
    terminal record.
    """
    from pi_dash.runner.models import AgentRunStatus

    now = timezone.now()

    try:
        signal = parse(text)
    except DoneSignalError as exc:
        run.error = f"done-signal parse error: {exc}"
        run.done_payload = None
        if not run.is_terminal:
            run.status = AgentRunStatus.FAILED
        run.ended_at = now
        run.save(update_fields=["error", "done_payload", "status", "ended_at"])
        return None

    run.done_payload = signal.payload
    run.error = ""
    if signal.status == "completed":
        run.status = AgentRunStatus.COMPLETED
    elif signal.status == "blocked":
        run.status = AgentRunStatus.BLOCKED
    elif signal.status == "noop":
        run.status = AgentRunStatus.COMPLETED
    run.ended_at = now
    run.save(update_fields=["done_payload", "error", "status", "ended_at"])
    return signal
