# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared session-open / hello-apply service.

Carved out of ``consumers.py`` per ``.ai_design/move_to_https/tasks.md``
§2.1 so both the legacy WS Hello path and the new
``POST /runners/<rid>/sessions/`` endpoint apply the same logic.
"""

from __future__ import annotations

import logging
from datetime import datetime, timedelta
from typing import Any, Dict, Optional
from uuid import UUID

from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerLiveState,
    RunnerStatus,
)

logger = logging.getLogger(__name__)

OFFLINE_GRACE_SECS = 60

# Grace window protecting freshly-assigned runs from the reaper.
#
# The reaper runs on every long-poll request (`runner_session_poll`)
# *before* the outbox is drained for that same response. So the very poll
# that's about to deliver an Assign to the runner is the poll where the
# runner still legitimately reports `in_flight_run=null` (it hasn't seen
# the Assign yet — it's still in the outbox waiting for delivery).
# Without this grace, that race kills the run between assignment and
# delivery: the cloud reaps `assigned_at=now-Xs, status=ASSIGNED`, then
# hands the doomed Assign to the runner, who processes it normally and
# only discovers the row is tombstoned when it tries to send RunStarted.
#
# Symptom: `started_at > ended_at` on the failed AgentRun row, error
# "reaped by heartbeat: ... in_flight_run=(none) but cloud had this run
# marked busy", with no daemon restart in the journal.
#
# Sized to cover one full long-poll interval (default 25s) + runner-side
# processing of the Assign + the runner's *next* poll. 60s is generous
# without meaningfully delaying real reaps after a daemon crash, since
# crash detection comes from missing heartbeats anyway.
ASSIGN_DELIVERY_GRACE_SECS = 60


def apply_hello(runner: Runner, body: Dict[str, Any]) -> None:
    """Update runner metadata + reap stale busy runs.

    ``body`` is the session-open / Hello payload. Persists ``os``,
    ``arch``, ``version``, and bumps ``last_heartbeat_at``.
    """
    runner.os = body.get("os", "") or runner.os
    runner.arch = body.get("arch", "") or runner.arch
    runner.runner_version = body.get("version", "") or runner.runner_version
    runner.last_heartbeat_at = timezone.now()
    runner.save(
        update_fields=["os", "arch", "runner_version", "last_heartbeat_at"]
    )
    # Session open redelivers ASSIGNED / WAITING_FOR_WORKTREE runs the
    # restarted daemon no longer reports (design §6.3) — reaping them here
    # would fail the very runs ``build_session_open_redeliver`` is about to
    # push back, so they are excluded on this path only. The poll path keeps
    # the default: an alive daemon that lost such a run is still reaped.
    reap_stale_busy_runs(runner, body, exclude_redeliverable=True)


def reap_stale_busy_runs(
    runner: Runner, body: Dict[str, Any], *, exclude_redeliverable: bool = False
) -> None:
    """Cancel BUSY runs the daemon no longer claims.

    WAITING_FOR_WORKTREE runs are covered automatically via
    ``BUSY_STATUSES`` membership and need no special-casing here (design
    §6.2): a single-tenant runner reports its queued run as
    ``in_flight_run`` while it waits, so it is excluded below; a daemon that
    truly lost the run stops reporting it and is correctly reaped, exactly as
    for RUNNING.

    ``exclude_redeliverable=True`` (session-open only) spares ASSIGNED /
    WAITING_FOR_WORKTREE runs so ``build_session_open_redeliver`` can push
    them back to the freshly-restarted daemon instead of failing them.
    """
    from django.db import transaction

    from pi_dash.runner.services.matcher import (
        BUSY_STATUSES,
        drain_for_runner_by_id,
        drain_pod_by_id,
    )

    now = timezone.now()
    ts_raw = body.get("ts")
    try:
        heartbeat_ts = (
            datetime.fromisoformat(ts_raw.replace("Z", "+00:00"))
            if isinstance(ts_raw, str)
            else now
        )
    except (ValueError, AttributeError):
        heartbeat_ts = now
    heartbeat_ts = min(heartbeat_ts, now)
    heartbeat_ts = max(heartbeat_ts, now - timedelta(seconds=OFFLINE_GRACE_SECS))

    in_flight = body.get("in_flight_run")
    in_flight_id: Optional[str] = None
    if in_flight:
        try:
            in_flight_id = str(UUID(str(in_flight)))
        except (ValueError, AttributeError):
            in_flight_id = None

    # Effective cutoff: a run is only stale if it was assigned BOTH
    # before the runner's reported heartbeat AND more than
    # ASSIGN_DELIVERY_GRACE_SECS ago. The min() picks the stricter
    # (earlier) of the two — `assigned_at < cutoff` is satisfied iff
    # both individual conditions are.
    assignment_cutoff = now - timedelta(seconds=ASSIGN_DELIVERY_GRACE_SECS)
    effective_cutoff = min(heartbeat_ts, assignment_cutoff)

    reapable_statuses = tuple(BUSY_STATUSES)
    if exclude_redeliverable:
        redeliverable = (
            AgentRunStatus.ASSIGNED,
            AgentRunStatus.WAITING_FOR_WORKTREE,
        )
        reapable_statuses = tuple(
            s for s in reapable_statuses if s not in redeliverable
        )

    stale = AgentRun.objects.filter(
        runner=runner,
        status__in=reapable_statuses,
        assigned_at__lt=effective_cutoff,
    )
    if in_flight_id:
        stale = stale.exclude(id=in_flight_id)
    reaped = list(stale.values_list("id", "pod_id"))
    if not reaped:
        return

    AgentRun.objects.filter(id__in=[rid for rid, _ in reaped]).update(
        status=AgentRunStatus.FAILED,
        ended_at=now,
        error=(
            "reaped by heartbeat: runner reported in_flight_run="
            f"{in_flight_id or '(none)'} but cloud had this run marked busy"
        ),
    )
    pod_ids = {pid for _, pid in reaped if pid is not None}
    runner_id = runner.id

    def _drain_after_commit(rid=runner_id, pids=pod_ids):
        drain_for_runner_by_id(rid)
        for pid in pids:
            drain_pod_by_id(pid)

    transaction.on_commit(_drain_after_commit)


# ---------------------------------------------------------------------------
# Per-active-run observability snapshot ingestion
# ---------------------------------------------------------------------------
#
# Lives on the poll path only (`apply_hello` is unchanged) — see
# `.ai_design/runner_agent_bridge/design.md` §4.2 / §4.5.2.
#
# Wire shape carries one envelope key (`observed_run_id`) plus optional
# scalar fields. `observed_run_id` *change* triggers a full wipe of the
# row's snapshot fields before applying the incoming values; missing
# scalars on a same-run poll are NOT overwritten (a stale poll never NULLs
# out a known-good value).

# Snapshot fields stored on RunnerLiveState. Does NOT include
# `observed_run_id` (that field drives the wipe, it is not a wipe target).
# Tokens travel as a nested `tokens.{input,output,total}` object and the
# selected LLM travels as top-level `model`; both are unpacked into flat
# columns by the upsert.
SNAPSHOT_FIELDS = (
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "llm_model",
    "turn_count",
)


def parse_optional_uuid(raw: Any) -> Optional[UUID]:
    """Return a UUID for present-and-valid values, ``None`` for missing /
    explicit-null. Raises ``ValueError`` for malformed UUID strings so the
    caller can decide to skip the entire poll's observability ingestion.
    """
    if raw is None:
        return None
    if isinstance(raw, UUID):
        return raw
    return UUID(str(raw))


def upsert_runner_live_state(
    runner: Runner, status_entry: Dict[str, Any]
) -> None:
    """Apply the volatile observability snapshot from a poll body.

    ``status_entry`` is the dict the poll handler reads as
    ``body['status']``. Missing fields are left as-is on the existing
    row — a stale poll never NULLs out a known-good value. A poll that
    carries a different ``observed_run_id`` than the row's current value
    persists a full wipe of every snapshot field before applying the
    incoming values, ensuring no cross-run carryover.

    No-op when the body has no observability fields at all (pre-flag
    runner). Malformed ``observed_run_id`` values are logged and skipped
    rather than raised so a buggy runner can't take down the cloud's
    poll handler.
    """
    if not isinstance(status_entry, dict) or not status_entry:
        return
    has_snapshot = "observed_run_id" in status_entry or any(
        key in status_entry for key in (*SNAPSHOT_FIELDS, "tokens", "model")
    )
    if not has_snapshot:
        return

    state, _ = RunnerLiveState.objects.get_or_create(runner=runner)

    try:
        incoming_run_id = parse_optional_uuid(status_entry.get("observed_run_id"))
    except (ValueError, TypeError):
        logger.warning(
            "ignoring runner live-state update for %s: invalid observed_run_id %r",
            runner.id,
            status_entry.get("observed_run_id"),
        )
        return

    update_fields: list[str] = []

    if (
        "observed_run_id" in status_entry
        and state.observed_run_id != incoming_run_id
    ):
        # New run, or idle/null after a completed run. Persist the full
        # wipe, not just the fields present on this poll.
        for f in SNAPSHOT_FIELDS:
            setattr(state, f, None)
        update_fields.extend(SNAPSHOT_FIELDS)
        state.observed_run_id = incoming_run_id
        update_fields.append("observed_run_id")

    for f in SNAPSHOT_FIELDS:
        if f in status_entry:
            setattr(state, f, status_entry[f])
            update_fields.append(f)

    if "tokens" in status_entry:
        tokens = status_entry["tokens"] or {}
        state.input_tokens = tokens.get("input")
        state.output_tokens = tokens.get("output")
        state.total_tokens = tokens.get("total")
        update_fields.extend(["input_tokens", "output_tokens", "total_tokens"])
    if "model" in status_entry:
        model = str(status_entry.get("model") or "").strip()
        state.llm_model = model[:128] or None
        update_fields.append("llm_model")

    if update_fields:
        state.save(
            update_fields=sorted(set(update_fields)) + ["updated_at"]
        )


# ``Runner.free_worktrees`` is an IntegerField; Postgres rejects anything
# above the signed-int32 ceiling, so out-of-range reports are clamped
# rather than allowed to 500 the poll handler.
FREE_WORKTREES_MAX = 2**31 - 1


def parse_free_worktrees(raw: Any) -> Optional[int]:
    """Coerce a reported ``free_worktrees`` hint to a non-negative int.

    Returns ``None`` for a missing or malformed value so the poll handler
    leaves the stored hint untouched (a pre-feature runner omits the field;
    a malformed one should never wipe a known-good value). Values beyond the
    column's int32 range are clamped rather than 500ing the poll. See
    ``.ai_design/worktree_pooling/design.md`` §6.4.
    """
    if raw is None:
        return None
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return None
    if value < 0:
        return None
    return min(value, FREE_WORKTREES_MAX)


def build_session_open_redeliver(
    runner: Runner, in_flight_run_id: Optional[str]
) -> Optional[Dict[str, Any]]:
    """Return an ``Assign``-shaped payload to redeliver at session open.

    After a daemon restart the runner's local worktree queue is gone, so a
    run the cloud still has assigned to it (``ASSIGNED`` /
    ``WAITING_FOR_WORKTREE``) is invisible to the daemon. When the runner
    re-opens its session without reporting that run as ``in_flight_run``, the
    cloud pushes it back so the daemon can re-attempt a lease (start or
    re-enqueue). See ``.ai_design/worktree_pooling/design.md`` §6.3.

    At most one such run exists per runner (single-tenancy); the oldest is
    chosen. ``RUNNING`` runs are deliberately excluded — a lost RUNNING run is
    the heartbeat reaper's job, not redelivery's. Returns ``None`` when there
    is nothing to redeliver.
    """
    from pi_dash.runner.services.matcher import _build_assign_msg

    skip_id: Optional[str] = None
    if in_flight_run_id:
        try:
            skip_id = str(UUID(str(in_flight_run_id)))
        except (ValueError, AttributeError):
            skip_id = None

    qs = AgentRun.objects.filter(
        runner=runner,
        status__in=(
            AgentRunStatus.ASSIGNED,
            AgentRunStatus.WAITING_FOR_WORKTREE,
        ),
    )
    if skip_id:
        qs = qs.exclude(id=skip_id)
    run = qs.order_by("assigned_at", "created_at").first()
    if run is None:
        return None
    return _build_assign_msg(run)


def mark_runner_online(runner_id: UUID | str) -> None:
    Runner.objects.filter(pk=runner_id).update(
        status=RunnerStatus.ONLINE, last_heartbeat_at=timezone.now()
    )


def mark_runner_offline(runner_id: UUID | str) -> None:
    Runner.objects.filter(pk=runner_id).exclude(
        status=RunnerStatus.REVOKED
    ).update(status=RunnerStatus.OFFLINE)


def resolve_runner_project_slug(runner: Runner) -> Optional[str]:
    """Return ``runner.pod.project.identifier`` or ``None``."""
    r = (
        Runner.objects.select_related("pod__project")
        .filter(pk=runner.pk)
        .first()
    )
    if r is None or r.pod_id is None:
        return None
    project = r.pod.project
    if project is None:
        return None
    return project.identifier


def build_resume_ack(runner: Runner, run_id: str) -> Optional[Dict[str, Any]]:
    """Return a ``resume_ack`` payload for an in-flight run, or ``None``.

    See ``consumers.RunnerConsumer._resume_run``: if the run does not
    exist, send a ``cancel`` instead; if it has terminated, send
    ``cancel`` with the terminal status.
    """
    from pi_dash.runner.models import AgentRunEvent

    run = AgentRun.objects.filter(id=run_id, runner=runner).first()
    if run is None:
        return {
            "type": "cancel",
            "run_id": str(run_id),
            "reason": "unknown_run_on_reconnect",
        }
    if run.is_terminal:
        return {
            "type": "cancel",
            "run_id": str(run_id),
            "reason": f"run_already_{run.status}",
        }
    last_seq = (
        AgentRunEvent.objects.filter(agent_run_id=run_id)
        .order_by("-seq")
        .values_list("seq", flat=True)
        .first()
    )
    return {
        "type": "resume_ack",
        "run_id": str(run_id),
        "last_seq": last_seq,
        "status": run.status,
        "thread_id": run.thread_id,
    }
