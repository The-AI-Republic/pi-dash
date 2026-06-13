# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop dispatch — turn one claimed, eligible target into an assistant turn.

Mirrors the chat message POST handler (``assistant/views/messages.py``) minus
HTTP, plus hidden-thread management with rotation. From
``run_assistant_turn.delay`` onward, nothing is loop-specific — the stock
assistant runtime resolves the user, credentials, history, limits, and
finalization. See ``.ai_design/loop_project_management/design.md`` §7.5.
"""

from __future__ import annotations

import logging

from django.conf import settings
from django.db import transaction
from django.utils import timezone

from pi_dash.assistant.errors import MAX_THREAD_MESSAGES
from pi_dash.assistant.models import (
    AssistantMessage,
    AssistantThread,
    AssistantTurn,
    MessageKind,
    MessageStatus,
    ThreadKind,
    TurnStatus,
)
from pi_dash.assistant.runtime import events
from pi_dash.assistant.tasks import run_assistant_turn
from pi_dash.db.models import LoopTarget, SkipReason

logger = logging.getLogger("pi_dash.worker")


def _ensure_thread(target: LoopTarget) -> AssistantThread:
    """Return a usable hidden loop thread for this target, rotating when the
    current one is near the per-thread message cap.

    Rotation resets the run's cross-run memory — acceptable, because job prompts
    must not depend on memory for correctness, only benefit from it.
    """
    headroom = int(getattr(settings, "LOOP_ROTATION_HEADROOM", 30))
    threshold = max(1, MAX_THREAD_MESSAGES - headroom)

    current = None
    if target.thread_id is not None:
        current = (
            AssistantThread.objects.filter(pk=target.thread_id, kind=ThreadKind.LOOP).first()
        )
    if current is not None:
        count = AssistantMessage.objects.filter(thread=current).count()
        if count < threshold:
            return current

    # Create a fresh thread; archive the old one (kept for admin history).
    fresh = AssistantThread.objects.create(
        workspace=target.workspace,
        user=target.user,
        kind=ThreadKind.LOOP,
        title=target.job.public_name[:255],
        is_archived=False,
    )
    if current is not None:
        AssistantThread.objects.filter(pk=current.pk).update(is_archived=True, updated_at=timezone.now())
    LoopTarget.objects.filter(pk=target.pk).update(thread=fresh, updated_at=timezone.now())
    target.thread = fresh
    return fresh


def dispatch_loop_turn(target_id: str) -> bool:
    """Create the hidden-thread turn for ``target_id`` and queue execution.

    Returns ``True`` when a turn was queued, ``False`` when skipped (a previous
    run is still in flight) or on an unexpected dispatch error.
    """
    try:
        with transaction.atomic():
            target = (
                LoopTarget.objects.select_for_update(of=("self",))
                .select_related("job", "workspace", "user")
                .get(pk=target_id)
            )
            thread = _ensure_thread(target)

            locked = AssistantThread.objects.select_for_update().get(pk=thread.pk)
            if locked.active_turn_id is not None:
                # Previous run still in flight — skip, don't queue (same policy
                # as the scheduler). The assistant stale-turn sweep guarantees
                # active_turn eventually clears even after a worker crash, so a
                # target can never wedge permanently.
                LoopTarget.objects.filter(pk=target.pk).update(
                    last_skipped_at=timezone.now(),
                    last_skip_reason=SkipReason.TURN_ACTIVE,
                    updated_at=timezone.now(),
                )
                return False

            turn = AssistantTurn.objects.create(thread=locked, status=TurnStatus.QUEUED)
            user_msg = events.create_message(
                locked,
                MessageKind.USER,
                turn=turn,
                display_content=target.job.prompt,
                status=MessageStatus.COMPLETED,
            )
            turn.user_message = user_msg
            turn.save(update_fields=["user_message"])
            locked.active_turn = turn
            locked.save(update_fields=["active_turn", "updated_at"])

            LoopTarget.objects.filter(pk=target.pk).update(
                last_run=turn, last_skip_reason="", updated_at=timezone.now()
            )

        transaction.on_commit(lambda tid=str(turn.id): run_assistant_turn.delay(tid))
        logger.info("loop.dispatch: queued turn=%s target=%s", turn.id, target_id)
        return True
    except LoopTarget.DoesNotExist:
        return False
    except Exception:  # noqa: BLE001 — record and move on; next occurrence retries
        logger.exception("loop.dispatch: unexpected error for target=%s", target_id)
        LoopTarget.objects.filter(pk=target_id).update(
            last_skipped_at=timezone.now(),
            last_skip_reason=SkipReason.DISPATCH_ERROR,
            updated_at=timezone.now(),
        )
        return False
