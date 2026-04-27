# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Channels consumer that owns one runner's WebSocket connection.

Runners authenticate with an ``Authorization: Bearer <runner_secret>`` header
on the HTTP upgrade request. The consumer joins a ``runner.<id>`` group so
other processes can push work via
:func:`pi_dash.runner.services.pubsub.send_to_runner`.
"""

from __future__ import annotations

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
    MachineToken,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services.pubsub import runner_group
from pi_dash.runner.services.tokens import hash_token

logger = logging.getLogger(__name__)

PROTOCOL_VERSION = 2
HEARTBEAT_INTERVAL_SECS = 25
OFFLINE_GRACE_SECS = 60


SEEN_MESSAGE_CACHE_SIZE = 512
MAX_SEQ_LOOKBACK = 128
# Upper bound on per-event JSON payload size. A rogue daemon could otherwise
# fill the DB with arbitrarily large blobs in AgentRunEvent.payload.
MAX_EVENT_PAYLOAD_BYTES = 64 * 1024
CLOSE_CODE_ROTATED = 4010


class RunnerConsumer(AsyncJsonWebsocketConsumer):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.runner: Optional[Runner] = None
        # Token (machine credential) authenticating the connection, if any.
        # Set when the daemon sends `X-Token-Id` + `Bearer <token_secret>`;
        # remains None on legacy v1 auth (per-runner runner_secret).
        self.token: Optional[MachineToken] = None
        self.group_name: Optional[str] = None
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
        """Send an outbound frame stamped with the wire envelope.

        The Rust runner's ``Envelope<T>`` requires ``v`` (protocol version)
        and ``mid`` (per-message UUID for dedupe) on every frame. Callers
        pass the logical fields (``type`` + type-specific keys); this helper
        adds the envelope. Any ``v``/``mid`` already in ``payload`` wins so
        tests can pin exact values if they need to.

        ``rid`` (per-runner routing discriminator) is added when the frame
        is bound to a specific runner — i.e. nearly always, while one
        connection still serves one runner. Connection-scoped frames
        (``ping``, ``bye``, future connection-wide ``revoke``) pass
        ``runner_scoped=False`` so the field is omitted, matching the
        runner's `Envelope::new` (no rid) vs `Envelope::for_runner`
        (with rid) split — see ``design.md`` §4.2 / §4.3.
        """
        frame: Dict[str, Any] = {
            "v": PROTOCOL_VERSION,
            "mid": str(uuid4()),
        }
        if runner_scoped and self.runner is not None and "rid" not in payload:
            frame["rid"] = str(self.runner.id)
        frame.update(payload)
        await self.send_json(frame)

    async def connect(self) -> None:
        auth = self._header("authorization")
        if not auth or not auth.lower().startswith("bearer "):
            await self.close(code=4401)
            return
        raw = auth.split(" ", 1)[1].strip()

        # Two auth modes:
        # 1. Token (multi-runner): X-Token-Id + Bearer <token_secret>. The
        #    daemon authenticates as the machine; runners under it come
        #    online via Hello frames. We resolve the token to its
        #    primary runner here for the single-runner-via-token path
        #    that ships first; multi-Hello support arrives in a later PR
        #    and replaces the primary-runner shortcut with a
        #    HashSet<Uuid> of authorised runner_ids.
        # 2. Legacy (single-runner): Authorization-only with the runner's
        #    own bearer secret. Existing v1 daemons use this path until
        #    they migrate.
        token_id_header = (self._header("x-token-id") or "").strip()
        if token_id_header:
            token, runner = await self._authenticate_via_token(
                token_id_header, raw
            )
            if token is None or runner is None:
                await self.close(code=4401)
                return
            self.token = token
        else:
            runner = await self._find_runner(raw)
            if runner is None:
                await self.close(code=4401)
                return
        if runner.status == RunnerStatus.REVOKED:
            await self.close(code=4403)
            return
        # Protocol check — log on mismatch, but tolerate garbage headers so a
        # malformed ``X-Runner-Protocol`` doesn't kill the connection.
        proto_raw = (self._header("x-runner-protocol") or "").strip()
        if proto_raw:
            try:
                proto_int = int(proto_raw)
            except ValueError:
                logger.warning(
                    "runner %s sent non-numeric protocol header %r",
                    runner.id,
                    proto_raw,
                )
            else:
                if proto_int != PROTOCOL_VERSION:
                    logger.warning(
                        "runner %s protocol mismatch (server=%s, client=%s)",
                        runner.id,
                        PROTOCOL_VERSION,
                        proto_int,
                    )
        self.runner = runner
        self.group_name = runner_group(runner.id)
        await self.channel_layer.group_add(self.group_name, self.channel_name)
        await self.accept()
        await self._mark_online(runner.id)
        await self._send_envelope({
            "type": "welcome",
            "server_time": timezone.now().isoformat(),
            "heartbeat_interval_secs": HEARTBEAT_INTERVAL_SECS,
            "protocol_version": PROTOCOL_VERSION,
        })

    async def disconnect(self, code: int) -> None:
        if self.group_name is not None:
            await self.channel_layer.group_discard(self.group_name, self.channel_name)
        if self.runner is not None:
            await self._mark_offline(self.runner.id)

    async def receive_json(self, content: Dict[str, Any], **_: Any) -> None:
        mtype = content.get("type")
        runner = self.runner
        if runner is None:
            return
        # Validate the per-runner routing discriminator if the daemon set
        # one. While one connection still authenticates as one runner, an
        # incoming ``rid`` must match the connection's authenticated runner
        # — anything else suggests a misbehaving daemon and we drop the
        # frame. Multi-runner support (cloud-side phase 1+2) will replace
        # this with a HashSet membership check.
        if not self._rid_matches(runner, content):
            logger.warning(
                "runner %s sent frame with mismatched rid %r; dropping",
                runner.id,
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

    @staticmethod
    def _rid_matches(runner: Runner, content: Dict[str, Any]) -> bool:
        """Validate envelope ``rid`` against the connection's authenticated
        runner. Frames without an ``rid`` are accepted (legacy / connection-
        scoped); frames with an ``rid`` must match the connection's runner.
        """
        rid = content.get("rid")
        if rid is None:
            return True
        try:
            return UUID(str(rid)) == runner.id
        except (ValueError, AttributeError):
            return False

    def _is_duplicate(self, content: Dict[str, Any]) -> bool:
        """LRU-bounded check on the wire ``mid`` so retries are idempotent."""
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
        """Enforce monotonic ``seq`` for ``run_event`` frames.

        Frames without a ``seq`` or a ``run_id`` pass through. A seq that is
        not strictly greater than the last-seen value is dropped and logged
        (duplicate or out-of-order); a gap (skip > 1) is logged but
        accepted — the transcript will simply be missing those events.
        """
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
        # Keep the map bounded per-connection.
        if len(self.last_seq_per_run) > MAX_SEQ_LOOKBACK:
            # Drop the oldest entry (dict preserves insertion order in 3.7+).
            self.last_seq_per_run.pop(next(iter(self.last_seq_per_run)))
        return True

    # ---- Inbound handlers ----

    async def on_hello(self, runner: Runner, msg: Dict[str, Any]) -> None:
        await sync_to_async(self._apply_hello)(runner, msg)
        in_flight = msg.get("in_flight_run")
        if in_flight:
            await self._resume_run(runner, str(in_flight))

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
        """Codex tool-call yield path. Cloud parks the run as
        PAUSED_AWAITING_INPUT and triggers drain so the runner is free to
        pick up other pod work while waiting for a human reply.

        Claude's yield arrives via ``on_run_completed`` with a
        ``pi-dash-done`` fenced block whose status is ``"paused"``; the
        done-signal parser handles that path separately.
        """
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
        """Acknowledge an in-flight run on reconnect.

        Loads the run, confirms it still belongs to this runner, and sends a
        ``resume_ack`` with the last seq we persisted so the daemon can avoid
        re-sending already-stored events. If the run was finalized during the
        disconnect window we send a ``cancel`` instead so the daemon tears
        down its local bridge cleanly.
        """
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
            # Prime the local seq map so dupes of already-persisted events drop.
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
        payload = event.get("payload") or {}
        try:
            await self._send_envelope(payload)
        except Exception:
            logger.exception("runner %s send failed", self.runner.id if self.runner else "?")

    async def runner_close(self, event: Dict[str, Any]) -> None:
        """Force-close this WS (e.g. after credential rotation).

        The daemon is expected to reconnect with the new secret.
        """
        await self.close(code=int(event.get("code") or CLOSE_CODE_ROTATED))

    # ---- Sync helpers (DB-bound) ----

    @staticmethod
    async def _find_runner(raw: str) -> Optional[Runner]:
        hashed = hash_token(raw)
        return await sync_to_async(
            lambda: Runner.objects.filter(credential_hash=hashed).first()
        )()

    @staticmethod
    async def _authenticate_via_token(
        token_id_raw: str, secret_raw: str
    ) -> tuple[Optional[MachineToken], Optional[Runner]]:
        """Look up the MachineToken by id and verify its secret.

        Returns ``(token, primary_runner)`` on success, ``(None, None)``
        on any failure (unknown id, mismatched secret, revoked token, no
        runners under it). Constant-time secret comparison via the same
        ``hash_token`` HMAC scheme used for runner_secret.

        For now we resolve the token to a single "primary runner" — the
        oldest-created Runner under the token. This keeps the rest of
        the consumer's per-runner code paths unchanged while phase 1 of
        cloud token auth ships. Multi-Hello support replaces this with a
        connection-scoped ``authorised_runner_ids`` set populated from
        ``token.runners`` and updated as Hello frames arrive.
        """
        try:
            token_id = UUID(token_id_raw)
        except (ValueError, AttributeError):
            return None, None
        secret_hashed = hash_token(secret_raw)

        def _lookup() -> tuple[Optional[MachineToken], Optional[Runner]]:
            token = MachineToken.objects.filter(
                id=token_id,
                secret_hash=secret_hashed,
                revoked_at__isnull=True,
            ).first()
            if token is None:
                return None, None
            primary = (
                Runner.objects.filter(machine_token=token)
                .order_by("created_at")
                .first()
            )
            return token, primary

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

    def _apply_hello(self, runner: Runner, msg: Dict[str, Any]) -> None:
        updates = ["os", "arch", "runner_version", "last_heartbeat_at"]
        runner.os = msg.get("os", "") or runner.os
        runner.arch = msg.get("arch", "") or runner.arch
        runner.runner_version = msg.get("version", "") or runner.runner_version
        runner.last_heartbeat_at = timezone.now()
        runner.save(update_fields=updates)

    def _apply_heartbeat(self, runner: Runner, msg: Dict[str, Any]) -> None:
        runner.mark_heartbeat()

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
        """Cloud reaction to a ResumeUnavailable failure (§6.3 of design).

        The agent CLI's session store no longer has the requested thread.
        Drop the pin so any runner can take it, drop the resume hint by
        nulling thread_id (so the next dispatch builds it from
        parent_run.thread_id, which would be the same broken id — instead
        we walk one level up so the run starts fresh-context-from-issue),
        re-queue, and trigger drain.
        """
        run = AgentRun.objects.filter(id=run_id, runner=runner).first()
        if run is None:
            return
        run.status = AgentRunStatus.QUEUED
        run.runner = None
        run.pinned_runner = None
        run.assigned_at = None
        # parent_run.thread_id is what _build_assign_msg reads; nulling
        # this run's thread_id is irrelevant. To prevent the cloud from
        # handing the same dead session id to the next runner, null the
        # parent's thread_id too — the parent is paused/terminal, this
        # is fine. The handoff comment carries the human-readable state.
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
        # Park the run; do NOT set ended_at (paused is non-terminal).
        AgentRun.objects.filter(id=run_id, runner=runner).update(
            status=AgentRunStatus.PAUSED_AWAITING_INPUT,
            done_payload=payload,
        )
        # Surface the agent's question as an issue comment so the human can
        # see it and reply (which then re-triggers continuation).
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
            # Agent payload is untrusted (the upstream prompt can shape it),
            # so escape with format_html before persisting to comment_html.
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

        # Two follow-ups: (1) sweep for non-bot comments that arrived while
        # this run was RUNNING — ``post_save(IssueComment)`` skipped them
        # with reason='prior-run-active'; pause is the symmetric
        # opportunity to pick them up. (2) Kick the dispatcher so the now-
        # idle runner can take other pod work (including any R_next the
        # sweep just created).
        from django.db import transaction

        from pi_dash.orchestration.service import maybe_continue_after_terminate
        from pi_dash.runner.services.matcher import drain_for_runner_by_id

        def _sweep_and_drain(rid=run_id, runner_id=runner.id):
            paused = AgentRun.objects.filter(pk=rid).first()
            if paused is not None:
                try:
                    maybe_continue_after_terminate(paused)
                except Exception:
                    logger.exception(
                        "orchestration.error: pause sweep failed for run %s",
                        rid,
                    )
            drain_for_runner_by_id(runner_id)

        transaction.on_commit(_sweep_and_drain)

    def _finalize_run(
        self,
        runner: Runner,
        msg: Dict[str, Any],
        new_status: AgentRunStatus,
    ) -> None:
        run_id = msg.get("run_id")
        if not run_id:
            return
        # Special-case resume-unavailable: don't terminate the chain, drop
        # the pin and re-queue without the resume hint so a fresh-context
        # dispatch can pick it up. See §6.3 of the design doc.
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

        # The runner just freed up. Two follow-up actions, both gated on
        # the terminal state having committed (otherwise a new assign
        # would race with the update):
        #
        # 1. Sweep for non-bot comments that arrived while this run was
        #    active. ``post_save(IssueComment)`` skipped them with
        #    reason='prior-run-active'; this is where they get picked up.
        # 2. Drain the pod (and prefer this just-freed runner) so any
        #    QUEUED work — including a follow-up R_next created by the
        #    sweep above — can dispatch.
        from django.db import transaction

        from pi_dash.orchestration.service import maybe_continue_after_terminate
        from pi_dash.runner.services.matcher import (
            drain_for_runner_by_id,
            drain_pod_by_id,
        )

        def _sweep_and_drain(rid=run_id, runner_id=runner.id, pod_id=runner.pod_id):
            run = AgentRun.objects.filter(pk=rid).first()
            if run is not None:
                try:
                    maybe_continue_after_terminate(run)
                except Exception:
                    logger.exception(
                        "orchestration.error: terminate sweep failed for run %s",
                        rid,
                    )
            drain_for_runner_by_id(runner_id)
            if pod_id is not None:
                drain_pod_by_id(pod_id)

        transaction.on_commit(_sweep_and_drain)

    # ---- misc ----

    def _header(self, name: str) -> Optional[str]:
        """Extract a header from the scope, case-insensitive."""
        headers = self.scope.get("headers") or []
        for key, value in headers:
            if key.decode().lower() == name:
                return value.decode()
        return None

    async def encode_json(self, content: Any) -> str:
        return json.dumps(content, default=str)
