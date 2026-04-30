# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Channels consumer for a dev-machine WebSocket.

The daemon authenticates as a Connection on the upgrade request:

    Authorization: Bearer <connection_secret>
    X-Connection-Id: <uuid>

After upgrade, individual runners come online with ``Hello { runner_id }``
frames; the consumer joins ``runner.<id>`` pubsub groups and routes
inbound/outbound work by ``rid`` on each frame.
"""

from __future__ import annotations

import asyncio
import json
import logging
from collections import OrderedDict
from typing import Any, Dict, Optional
from uuid import UUID, uuid4

from asgiref.sync import sync_to_async
from channels.generic.websocket import AsyncJsonWebsocketConsumer
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunEvent,
    AgentRunStatus,
    ApprovalKind,
    ApprovalRequest,
    ApprovalStatus,
    Connection,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services.pubsub import runner_group
from pi_dash.runner.services.tokens import hash_token

logger = logging.getLogger(__name__)

PROTOCOL_VERSION = 3
HEARTBEAT_INTERVAL_SECS = 25
OFFLINE_GRACE_SECS = 60


SEEN_MESSAGE_CACHE_SIZE = 512
MAX_SEQ_LOOKBACK = 128
MAX_EVENT_PAYLOAD_BYTES = 64 * 1024
CLOSE_CODE_ROTATED = 4010
# Close code for a token-mode connection that opened the WS but never
# brought any runner online via Hello within HELLO_DEADLINE_SECS. Keeps
# half-open daemons from holding consumer slots indefinitely.
CLOSE_CODE_HELLO_TIMEOUT = 4408
HELLO_DEADLINE_SECS = 30


def _update_scheduler_binding_on_terminate(run: AgentRun) -> None:
    """Update ``SchedulerBinding.last_error`` after a project-scoped run
    reaches a terminal state.

    ``binding.last_run`` already points at this run (set at dispatch
    time), so ``last_run.status`` is the operator-facing source of truth
    for "did the last tick succeed." This helper only writes the
    short-circuit ``last_error`` string for terminal-failure runs.

    See ``.ai_design/project_scheduler/design.md`` §6.5.
    """
    binding = run.scheduler_binding
    if binding is None:
        return
    from pi_dash.db.models.scheduler import LAST_ERROR_MAX_LEN

    if run.status == AgentRunStatus.COMPLETED:
        if binding.last_error:
            binding.last_error = ""
            binding.save(update_fields=["last_error", "updated_at"])
    elif run.status in (AgentRunStatus.FAILED, AgentRunStatus.CANCELLED):
        msg = (run.error or run.status)[:LAST_ERROR_MAX_LEN]
        if binding.last_error != msg:
            binding.last_error = msg
            binding.save(update_fields=["last_error", "updated_at"])


class RunnerConsumer(AsyncJsonWebsocketConsumer):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        # The Connection authenticating this WS. Set at connect time and
        # immutable for the WS lifetime.
        self.connection: Optional[Connection] = None
        # Runners brought online via Hello, keyed by runner_id. Frame
        # routing — both inbound (`receive_json`'s rid lookup) and
        # outbound (`_send_envelope`'s rid stamping) — picks runners
        # out of this map.
        self.authorised_runners: Dict[UUID, Runner] = {}
        # Each authorised runner has its own pubsub group; a connection
        # is joined to N of them. Tracked so disconnect can leave them
        # all cleanly.
        self.group_names: list[str] = []
        # Hello watchdog — closes the connection if no runner comes online
        # within HELLO_DEADLINE_SECS. Cancelled on first successful Hello
        # and on disconnect so a half-open daemon doesn't tie up a slot.
        self._hello_deadline_task: Optional[asyncio.Task] = None
        # Per-connection dedupe cache of message_ids we've already applied.
        # LRU-bounded so a misbehaving runner can't grow us unboundedly.
        self.seen_messages: "OrderedDict[str, None]" = OrderedDict()
        # Per-run last-seen seq; used to drop duplicates and log gaps.
        self.last_seq_per_run: Dict[str, int] = {}

    async def _send_envelope(
        self,
        payload: Dict[str, Any],
        *,
        runner_scoped: bool = True,
    ) -> None:
        """Send an outbound frame with a ``v`` + ``mid`` envelope.

        The Rust runner's ``Envelope<T>`` requires ``v`` (protocol version)
        and ``mid`` (per-message UUID for dedupe) on every frame. Callers
        pass logical fields; this helper adds the envelope. Connection-
        scoped frames (``ping``, ``bye``, ``revoke``) pass
        ``runner_scoped=False`` so ``rid`` is omitted.
        """
        frame: Dict[str, Any] = {
            "v": PROTOCOL_VERSION,
            "mid": str(uuid4()),
        }
        frame.update(payload)
        await self.send_json(frame)

    async def connect(self) -> None:
        auth = self._header("authorization")
        if not auth or not auth.lower().startswith("bearer "):
            await self.close(code=4401)
            return
        raw = auth.split(" ", 1)[1].strip()
        connection_id_header = (self._header("x-connection-id") or "").strip()
        if not connection_id_header:
            await self.close(code=4401)
            return
        connection = await self._authenticate_connection(connection_id_header, raw)
        if connection is None:
            await self.close(code=4401)
            return
        self.connection = connection
        # Protocol check — log on mismatch, but tolerate garbage so a
        # malformed ``X-Runner-Protocol`` doesn't kill the connection.
        proto_raw = (self._header("x-runner-protocol") or "").strip()
        if proto_raw:
            try:
                proto_int = int(proto_raw)
            except ValueError:
                logger.warning(
                    "connection %s sent non-numeric protocol header %r",
                    connection.id,
                    proto_raw,
                )
            else:
                if proto_int != PROTOCOL_VERSION:
                    logger.warning(
                        "connection %s protocol mismatch (server=%s, client=%s)",
                        connection.id,
                        PROTOCOL_VERSION,
                        proto_int,
                    )
        await self.accept()
        # Arm the Hello watchdog. Without this, a daemon that completes
        # the upgrade and never sends Hello holds a consumer slot forever.
        self._hello_deadline_task = asyncio.create_task(
            self._enforce_hello_deadline()
        )

    async def disconnect(self, code: int) -> None:
        # Cancel the Hello watchdog if it's still pending — it would
        # otherwise issue a duplicate close on an already-disconnected
        # consumer.
        if self._hello_deadline_task is not None:
            self._hello_deadline_task.cancel()
            self._hello_deadline_task = None
        for group in self.group_names:
            await self.channel_layer.group_discard(group, self.channel_name)
        for runner_id in list(self.authorised_runners.keys()):
            await self._mark_offline(runner_id)

    async def receive_json(self, content: Dict[str, Any], **_: Any) -> None:
        mtype = content.get("type")
        if self.connection is None:
            return
        if mtype == "hello":
            await self._handle_hello(content)
            return
        if mtype == "bye":
            await self.close()
            return
        runner = self._resolve_inbound_runner(content)
        if runner is None:
            logger.warning(
                "connection %s sent %s frame with unknown rid %r; dropping",
                self.connection.id,
                mtype,
                content.get("rid"),
            )
            return
        if self._is_duplicate(content):
            logger.debug(
                "runner %s sent duplicate message %s; dropping",
                runner.id,
                content.get("mid"),
            )
            return
        if not self._seq_ok(runner, content):
            return
        handler = getattr(self, f"on_{mtype}", None)
        if handler is None:
            logger.debug("runner %s sent unknown type %s", runner.id, mtype)
            return
        try:
            await handler(runner, content)
        except Exception:
            logger.exception("error handling %s from runner %s", mtype, runner.id)

    def _resolve_inbound_runner(self, content: Dict[str, Any]) -> Optional[Runner]:
        rid = content.get("rid")
        if rid is None:
            return None
        try:
            rid_uuid = UUID(str(rid))
        except (ValueError, AttributeError):
            return None
        return self.authorised_runners.get(rid_uuid)

    async def _handle_hello(self, content: Dict[str, Any]) -> None:
        """Bring a runner online over an authenticated connection.

        Steps:
        1. Parse ``runner_id`` from the body and sanity-check the envelope's
           ``rid`` agrees.
        2. Verify the runner is owned by this connection and not revoked.
        3. Mark online, join the per-runner pubsub group, store in
           authorised_runners, send a runner-scoped Welcome.
        """
        if self.connection is None:
            return
        body_runner_id = content.get("runner_id")
        try:
            runner_id = UUID(str(body_runner_id))
        except (ValueError, AttributeError):
            logger.warning(
                "connection %s sent Hello with invalid runner_id %r",
                self.connection.id,
                body_runner_id,
            )
            return
        rid = content.get("rid")
        if rid is not None:
            try:
                rid_uuid = UUID(str(rid))
            except (ValueError, AttributeError):
                logger.warning(
                    "connection %s sent Hello with malformed envelope rid %r",
                    self.connection.id,
                    rid,
                )
                return
            if rid_uuid != runner_id:
                logger.warning(
                    "connection %s sent Hello with mismatched rid %s vs runner_id %s",
                    self.connection.id,
                    rid_uuid,
                    runner_id,
                )
                await self._send_envelope(
                    {
                        "type": "remove_runner",
                        "rid": str(runner_id),
                        "runner_id": str(runner_id),
                        "reason": "hello_rid_mismatch",
                    },
                    runner_scoped=False,
                )
                return

        runner = await self._resolve_connection_runner(self.connection, runner_id)
        if runner is None or runner.status == RunnerStatus.REVOKED:
            logger.warning(
                "connection %s tried to bring runner %s online but it is not owned/revoked",
                self.connection.id,
                runner_id,
            )
            await self._send_envelope(
                {
                    "type": "remove_runner",
                    "rid": str(runner_id),
                    "runner_id": str(runner_id),
                    "reason": "not_owned_or_revoked",
                },
                runner_scoped=False,
            )
            return

        body_project_slug = content.get("project_slug")
        if body_project_slug is not None:
            expected_slug = await self._resolve_runner_project_slug(runner)
            if expected_slug is None:
                logger.debug(
                    "connection %s Hello for runner %s claimed project_slug %r "
                    "but cloud could not resolve runner.pod.project; "
                    "skipping cross-check",
                    self.connection.id,
                    runner_id,
                    body_project_slug,
                )
            elif str(body_project_slug) != expected_slug:
                logger.warning(
                    "connection %s Hello for runner %s claimed project %r but cloud has %r",
                    self.connection.id,
                    runner_id,
                    body_project_slug,
                    expected_slug,
                )
                await self._send_envelope(
                    {
                        "type": "remove_runner",
                        "rid": str(runner_id),
                        "runner_id": str(runner_id),
                        "reason": "project_mismatch",
                    },
                    runner_scoped=False,
                )
                return

        if runner_id in self.authorised_runners:
            return

        self.authorised_runners[runner_id] = runner
        # First successful Hello disarms the watchdog — the connection has
        # proven it's a real daemon, not a half-open socket.
        if self._hello_deadline_task is not None:
            self._hello_deadline_task.cancel()
            self._hello_deadline_task = None
        group = runner_group(runner.id)
        self.group_names.append(group)
        await self.channel_layer.group_add(group, self.channel_name)
        await self._mark_online(runner.id)
        await sync_to_async(self._apply_hello)(runner, content)
        await self._touch_connection_seen()
        await sync_to_async(self._drain_after_online)(runner.id)
        await self._send_envelope(
            {
                "type": "welcome",
                "rid": str(runner.id),
                "server_time": timezone.now().isoformat(),
                "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
                "protocol_version": PROTOCOL_VERSION,
            }
        )
        in_flight = content.get("in_flight_run")
        if in_flight:
            await self._resume_run(runner, str(in_flight))

    def _is_duplicate(self, content: Dict[str, Any]) -> bool:
        mid = content.get("mid")
        if not mid:
            return False
        mid = str(mid)
        if mid in self.seen_messages:
            self.seen_messages.move_to_end(mid)
            return True
        self.seen_messages[mid] = None
        if len(self.seen_messages) > SEEN_MESSAGE_CACHE_SIZE:
            self.seen_messages.popitem(last=False)
        return False

    def _seq_ok(self, runner: Runner, content: Dict[str, Any]) -> bool:
        if content.get("type") != "run_event":
            return True
        run_id = str(content.get("run_id") or "")
        seq = content.get("seq")
        if not run_id or seq is None:
            return True
        try:
            seq = int(seq)
        except (TypeError, ValueError):
            return True
        last = self.last_seq_per_run.get(run_id)
        if last is not None and seq <= last:
            logger.info(
                "runner %s sent seq=%s <= last=%s for run %s; dropping",
                runner.id,
                seq,
                last,
                run_id,
            )
            return False
        if last is not None and seq > last + 1:
            logger.info(
                "runner %s sent seq=%s with gap after %s for run %s",
                runner.id,
                seq,
                last,
                run_id,
            )
        self.last_seq_per_run[run_id] = seq
        if len(self.last_seq_per_run) > MAX_SEQ_LOOKBACK:
            self.last_seq_per_run.pop(next(iter(self.last_seq_per_run)))
        return True

    # ---- Inbound handlers ----

    async def on_heartbeat(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._apply_heartbeat)(runner, msg)

    async def on_accept(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._apply_lifecycle)(
            runner, msg, AgentRunStatus.RUNNING
        )

    async def on_run_started(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._apply_run_started)(runner, msg)

    async def on_run_event(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._persist_event)(runner, msg)

    async def on_approval_request(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._persist_approval)(runner, msg)

    async def on_run_completed(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._finalize_run)(runner, msg, AgentRunStatus.COMPLETED)

    async def on_run_failed(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._finalize_run)(runner, msg, AgentRunStatus.FAILED)

    async def on_run_cancelled(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._finalize_run)(runner, msg, AgentRunStatus.CANCELLED)

    async def on_run_paused(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._handle_run_paused)(runner, msg)

    async def on_run_awaiting_reauth(
        self, runner: Runner, msg: Dict[str, Any]
    ) -> None:
        await sync_to_async(self._apply_lifecycle)(
            runner, msg, AgentRunStatus.AWAITING_REAUTH
        )

    async def on_run_resumed(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._apply_lifecycle)(runner, msg, AgentRunStatus.RUNNING)
        run_id = msg.get("run_id")
        if run_id:
            await self._resume_run(runner, str(run_id))

    async def _resume_run(self, runner: Runner, run_id: str) -> None:
        run = await sync_to_async(
            lambda: AgentRun.objects.filter(id=run_id, runner=runner).first()
        )()
        if run is None:
            await self._send_envelope({
                "type": "cancel",
                "run_id": run_id,
                "reason": "unknown_run_on_reconnect",
            })
            return
        if run.is_terminal:
            await self._send_envelope({
                "type": "cancel",
                "run_id": run_id,
                "reason": f"run_already_{run.status}",
            })
            return
        last_seq = await sync_to_async(self._last_seq_for_run)(run_id)
        await self._send_envelope({
            "type": "resume_ack",
            "run_id": run_id,
            "last_seq": last_seq,
            "status": run.status,
            "thread_id": run.thread_id,
        })
        if last_seq is not None:
            self.last_seq_per_run[run_id] = last_seq

    @staticmethod
    def _last_seq_for_run(run_id: str) -> Optional[int]:
        return (
            AgentRunEvent.objects.filter(agent_run_id=run_id)
            .order_by("-seq")
            .values_list("seq", flat=True)
            .first()
        )

    async def on_bye(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await self.close()

    # ---- Outbound — delivered via channels group_send ----

    async def runner_send(self, event: Dict[str, Any]) -> None:
        payload = dict(event.get("payload") or {})
        target_id = event.get("runner_id")
        if target_id and "rid" not in payload:
            payload["rid"] = str(target_id)
        try:
            await self._send_envelope(payload)
        except Exception:
            logger.exception("runner %s send failed", target_id or "?")

    async def runner_close(self, event: Dict[str, Any]) -> None:
        await self.close(code=int(event.get("code") or CLOSE_CODE_ROTATED))

    async def runner_revoke(self, event: Dict[str, Any]) -> None:
        reason = str(event.get("reason") or "connection revoked")
        try:
            await self._send_envelope(
                {"type": "revoke", "reason": reason},
                runner_scoped=False,
            )
        except Exception:
            logger.exception("failed to send revoke frame")
        await self.close(code=CLOSE_CODE_ROTATED)

    async def _enforce_hello_deadline(self) -> None:
        """Close a connection that opened the WS but never sent a Hello
        within ``HELLO_DEADLINE_SECS``.

        Cleared on first successful Hello (see ``_handle_hello``) and on
        disconnect, so a daemon that completes the upgrade and never
        introduces a runner doesn't tie up a consumer slot indefinitely.
        """
        try:
            await asyncio.sleep(HELLO_DEADLINE_SECS)
        except asyncio.CancelledError:
            return
        if self.connection is None or self.authorised_runners:
            return
        logger.warning(
            "connection %s opened WS but sent no Hello in %ds; closing",
            self.connection.id,
            HELLO_DEADLINE_SECS,
        )
        try:
            await self.close(code=CLOSE_CODE_HELLO_TIMEOUT)
        except Exception:
            # close() can race with disconnect; swallow rather than crash
            # the watchdog task.
            logger.debug("close after Hello timeout raised", exc_info=True)

    # ---- Sync helpers (DB-bound) ----

    @staticmethod
    async def _authenticate_connection(
        connection_id_raw: str, secret_raw: str
    ) -> Optional[Connection]:
        try:
            connection_id = UUID(connection_id_raw)
        except (ValueError, AttributeError):
            return None
        secret_hashed = hash_token(secret_raw)
        return await sync_to_async(
            lambda: Connection.objects.filter(
                id=connection_id,
                secret_hash=secret_hashed,
                revoked_at__isnull=True,
                enrolled_at__isnull=False,
            ).first()
        )()

    async def _touch_connection_seen(self) -> None:
        if self.connection is None:
            return
        connection_id = self.connection.id
        await sync_to_async(
            lambda: Connection.objects.filter(pk=connection_id).update(
                last_seen_at=timezone.now()
            )
        )()

    @staticmethod
    async def _resolve_connection_runner(
        connection: Connection, runner_id: UUID
    ) -> Optional[Runner]:
        return await sync_to_async(
            lambda: Runner.objects.filter(
                id=runner_id,
                connection=connection,
                revoked_at__isnull=True,
            ).first()
        )()

    @staticmethod
    async def _resolve_runner_project_slug(runner: Runner) -> Optional[str]:
        def _lookup() -> Optional[str]:
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

        return await sync_to_async(_lookup)()

    @staticmethod
    async def _mark_online(runner_id: UUID) -> None:
        await sync_to_async(
            lambda: Runner.objects.filter(pk=runner_id).update(
                status=RunnerStatus.ONLINE,
                last_heartbeat_at=timezone.now(),
            )
        )()

    @staticmethod
    async def _mark_offline(runner_id: UUID) -> None:
        await sync_to_async(
            lambda: Runner.objects.filter(pk=runner_id)
            .exclude(status=RunnerStatus.REVOKED)
            .update(status=RunnerStatus.OFFLINE)
        )()

    @staticmethod
    def _drain_after_online(runner_id: UUID) -> None:
        from django.db import transaction

        from pi_dash.runner.services.matcher import drain_for_runner_by_id

        transaction.on_commit(lambda rid=runner_id: drain_for_runner_by_id(rid))

    def _apply_hello(self, runner: Runner, msg: Dict[str, Any]) -> None:
        updates = ["os", "arch", "runner_version", "last_heartbeat_at"]
        runner.os = msg.get("os", "") or runner.os
        runner.arch = msg.get("arch", "") or runner.arch
        runner.runner_version = msg.get("version", "") or runner.runner_version
        runner.last_heartbeat_at = timezone.now()
        runner.save(update_fields=updates)
        self._reap_stale_busy_runs(runner, msg)

    def _apply_heartbeat(self, runner: Runner, msg: Dict[str, Any]) -> None:
        runner.mark_heartbeat()
        self._reap_stale_busy_runs(runner, msg)

    def _reap_stale_busy_runs(self, runner: Runner, msg: Dict[str, Any]) -> None:
        from datetime import datetime, timedelta

        from django.db import transaction

        from pi_dash.runner.services.matcher import (
            BUSY_STATUSES,
            drain_for_runner_by_id,
            drain_pod_by_id,
        )

        now = timezone.now()
        ts_raw = msg.get("ts")
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

        in_flight = msg.get("in_flight_run")
        in_flight_id: Optional[str] = None
        if in_flight:
            try:
                in_flight_id = str(UUID(str(in_flight)))
            except (ValueError, AttributeError):
                in_flight_id = None

        stale = AgentRun.objects.filter(
            runner=runner,
            status__in=BUSY_STATUSES,
            assigned_at__lt=heartbeat_ts,
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
                f"{in_flight_id or '(none)'} "
                "but cloud had this run marked busy"
            ),
        )

        logger.info(
            "consumer.heartbeat_reap: runner=%s reaped %d stale run(s)",
            runner.id,
            len(reaped),
        )

        pod_ids = {pid for _, pid in reaped if pid is not None}
        runner_id = runner.id

        def _drain_after_commit(rid=runner_id, pids=pod_ids):
            drain_for_runner_by_id(rid)
            for pid in pids:
                drain_pod_by_id(pid)

        transaction.on_commit(_drain_after_commit)

    def _apply_lifecycle(
        self,
        runner: Runner,
        msg: Dict[str, Any],
        new_status: AgentRunStatus,
    ) -> None:
        run_id = msg.get("run_id")
        if not run_id:
            return
        AgentRun.objects.filter(id=run_id, runner=runner).update(status=new_status)

    def _apply_run_started(self, runner: Runner, msg: Dict[str, Any]) -> None:
        run_id = msg.get("run_id")
        thread_id = msg.get("thread_id") or ""
        if not run_id:
            return
        AgentRun.objects.filter(id=run_id, runner=runner).update(
            status=AgentRunStatus.RUNNING,
            thread_id=thread_id,
            started_at=timezone.now(),
        )

    def _persist_event(self, runner: Runner, msg: Dict[str, Any]) -> None:
        run_id = msg.get("run_id")
        seq = int(msg.get("seq") or 0)
        kind = (msg.get("kind") or "")[:64]
        payload = msg.get("payload") or {}
        if not run_id or not kind:
            return
        try:
            encoded = json.dumps(payload, default=str)
        except (TypeError, ValueError):
            encoded = ""
        if len(encoded.encode("utf-8")) > MAX_EVENT_PAYLOAD_BYTES:
            logger.info(
                "runner %s sent oversized event payload (run=%s seq=%s); truncating",
                runner.id,
                run_id,
                seq,
            )
            payload = {
                "_truncated": True,
                "original_size_bytes": len(encoded.encode("utf-8")),
            }
        AgentRunEvent.objects.update_or_create(
            agent_run_id=run_id,
            seq=seq,
            defaults={"kind": kind, "payload": payload},
        )

    def _persist_approval(self, runner: Runner, msg: Dict[str, Any]) -> None:
        run_id = msg.get("run_id")
        approval_id = msg.get("approval_id")
        kind = (msg.get("kind") or ApprovalKind.OTHER).lower()
        kind_mapped = {
            "command_execution": ApprovalKind.COMMAND_EXECUTION,
            "file_change": ApprovalKind.FILE_CHANGE,
            "network_access": ApprovalKind.NETWORK_ACCESS,
        }.get(kind, ApprovalKind.OTHER)
        expires = msg.get("expires_at")
        ApprovalRequest.objects.update_or_create(
            id=approval_id,
            defaults={
                "agent_run_id": run_id,
                "kind": kind_mapped,
                "payload": msg.get("payload") or {},
                "reason": msg.get("reason") or "",
                "status": ApprovalStatus.PENDING,
                "expires_at": expires,
            },
        )
        AgentRun.objects.filter(id=run_id, runner=runner).update(
            status=AgentRunStatus.AWAITING_APPROVAL
        )

    def _handle_resume_unavailable(
        self, runner: Runner, run_id: str
    ) -> None:
        run = AgentRun.objects.filter(id=run_id, runner=runner).first()
        if run is None:
            return
        run.status = AgentRunStatus.QUEUED
        run.runner = None
        run.pinned_runner = None
        run.assigned_at = None
        if run.parent_run is not None and run.parent_run.thread_id:
            run.parent_run.thread_id = ""
            run.parent_run.save(update_fields=["thread_id"])
        run.save(
            update_fields=["status", "runner", "pinned_runner", "assigned_at"]
        )
        from django.db import transaction

        from pi_dash.runner.services.matcher import drain_pod_by_id

        if run.pod_id is not None:
            transaction.on_commit(
                lambda pid=run.pod_id: drain_pod_by_id(pid)
            )

    def _handle_run_paused(
        self, runner: Runner, msg: Dict[str, Any]
    ) -> None:
        run_id = msg.get("run_id")
        if not run_id:
            return
        payload = msg.get("payload") or {}
        AgentRun.objects.filter(id=run_id, runner=runner).update(
            status=AgentRunStatus.PAUSED_AWAITING_INPUT,
            done_payload=payload,
        )
        try:
            run = AgentRun.objects.select_related("work_item").get(id=run_id)
        except AgentRun.DoesNotExist:
            return
        if run.work_item_id is not None:
            from django.utils.html import format_html

            from pi_dash.orchestration.workpad import get_agent_system_user
            from pi_dash.db.models.issue import IssueComment

            question = (payload.get("autonomy") or {}).get("question_for_human")
            summary = payload.get("summary")
            body_parts: list[str] = []
            if question:
                body_parts.append(
                    format_html(
                        "<p><strong>Agent paused — question:</strong></p><p>{}</p>",
                        question,
                    )
                )
            if summary:
                body_parts.append(
                    format_html("<p><em>Summary so far:</em> {}</p>", summary)
                )
            if body_parts:
                IssueComment.objects.create(
                    issue=run.work_item,
                    project=run.work_item.project,
                    workspace=run.work_item.workspace,
                    actor=get_agent_system_user(),
                    comment_html="".join(body_parts),
                )

        from django.db import transaction

        from pi_dash.orchestration.scheduling import maybe_apply_deferred_pause
        from pi_dash.runner.services.matcher import drain_for_runner_by_id

        def _pause_and_drain(rid=run_id, runner_id=runner.id):
            paused = (
                AgentRun.objects.select_related(
                    "work_item",
                    "work_item__state",
                    "work_item__project",
                )
                .filter(pk=rid)
                .first()
            )
            if paused is not None:
                try:
                    maybe_apply_deferred_pause(paused)
                except Exception:
                    logger.exception(
                        "orchestration.error: deferred-pause failed for run %s",
                        rid,
                    )
            drain_for_runner_by_id(runner_id)

        transaction.on_commit(_pause_and_drain)

    def _finalize_run(
        self,
        runner: Runner,
        msg: Dict[str, Any],
        new_status: AgentRunStatus,
    ) -> None:
        run_id = msg.get("run_id")
        if not run_id:
            return
        if (
            new_status == AgentRunStatus.FAILED
            and msg.get("reason") == "resume_unavailable"
        ):
            self._handle_resume_unavailable(runner, run_id)
            return
        updates: Dict[str, Any] = {
            "status": new_status,
            "ended_at": timezone.now(),
        }
        if new_status == AgentRunStatus.COMPLETED:
            updates["done_payload"] = msg.get("done_payload")
        if new_status == AgentRunStatus.FAILED:
            updates["error"] = (msg.get("detail") or "")[:16000]
        AgentRun.objects.filter(id=run_id, runner=runner).update(**updates)

        from django.db import transaction

        from pi_dash.orchestration.scheduling import maybe_apply_deferred_pause
        from pi_dash.runner.services.matcher import (
            drain_for_runner_by_id,
            drain_pod_by_id,
        )

        def _pause_and_drain(rid=run_id, runner_id=runner.id, pod_id=runner.pod_id):
            run = (
                AgentRun.objects.select_related(
                    "work_item",
                    "work_item__state",
                    "work_item__project",
                    "scheduler_binding",
                )
                .filter(pk=rid)
                .first()
            )
            if run is not None:
                try:
                    maybe_apply_deferred_pause(run)
                except Exception:
                    logger.exception(
                        "orchestration.error: deferred-pause failed for run %s",
                        rid,
                    )
                # Scheduler-driven run finished: write last_error on the
                # binding so operators can see why a scheduler tick failed
                # without drilling into the AgentRun. ``binding.last_run``
                # was set at dispatch time and already points at this run,
                # so its `.status` is the source of truth — we only update
                # the short-circuit error string here.
                # See .ai_design/project_scheduler/design.md §6.5.
                if run.scheduler_binding_id is not None:
                    try:
                        _update_scheduler_binding_on_terminate(run)
                    except Exception:
                        logger.exception(
                            "scheduler.terminate_hook: failed for run %s",
                            rid,
                        )
            drain_for_runner_by_id(runner_id)
            if pod_id is not None:
                drain_pod_by_id(pod_id)

        transaction.on_commit(_pause_and_drain)

    # ---- misc ----

    def _header(self, name: str) -> Optional[str]:
        headers = self.scope.get("headers") or []
        for key, value in headers:
            if key.decode().lower() == name:
                return value.decode()
        return None

    async def encode_json(self, content: Any) -> str:
        return json.dumps(content, default=str)
