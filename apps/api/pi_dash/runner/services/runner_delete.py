# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared cloud-side runner-delete service.

The web-app delete endpoint (``DELETE /api/runners/<id>/``, session
auth) and the X-Api-Key v1 endpoint (``DELETE /api/v1/runners/<id>/``,
called by the local CLI) both delegate here so the two surfaces stay
in lockstep.

There are two valid termination modes, mirroring the spec the user
agreed on:

- ``purge_local=True`` (default for the cascade UX): the cloud
  emits a ``remove_runner`` control frame that tells the daemon to
  cancel its in-flight run, drop the per-runner data dir, *and*
  strip the matching ``[[runner]]`` block from ``config.toml``.
  The systemd unit is shared across runners and is deliberately
  untouched.

- ``purge_local=False``: the cloud emits a plain ``revoke`` frame
  (today's behavior). The daemon exits its RunnerLoop but leaves
  every local file alone — operators who want to wipe local state
  must run ``pidash runner remove`` themselves.

In both modes the cloud-side row is hard-deleted *after* the frame is
enqueued, the in-flight runs are cancelled via ``Runner.revoke``, and
the live session is evicted via ``close_runner_session``.
"""

from __future__ import annotations

from uuid import UUID

from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import DevMachine, MachineToken, Runner
from pi_dash.runner.services.pubsub import (
    close_runner_session,
    send_runner_remove,
    send_runner_revoke,
)


def delete_runner(runner: Runner, *, purge_local: bool) -> None:
    """Tear down ``runner`` cloud-side and (optionally) cascade to local.

    Caller is responsible for authentication + authorization.

    Order matters: the control frame is enqueued *before*
    ``runner.revoke()`` runs. ``revoke()`` revokes the active
    ``RunnerSession`` row, after which ``enqueue_for_runner`` would see
    no active session and divert the frame into the offline buffer —
    but the row is then hard-deleted, so future auth attempts return
    ``runner_not_found`` and the offline buffer never drains. Enqueue
    while the session is still alive so the daemon's in-flight
    long-poll can drain the frame; revoke + close + delete then run as
    the cleanup tail.

    The cleanup tail (revoke + close + delete) is wrapped in an outer
    ``transaction.atomic()`` so a failure between ``revoke()`` and the
    row delete can't leave the runner in a partial state
    (revoked-but-still-present). ``revoke()`` opens its own
    ``transaction.atomic()`` which nests as a savepoint;
    ``close_runner_session`` publishes to Redis from within the tx —
    that publish is not transactional, but the daemon already treats
    such frames as idempotent advisories, so a rollback after the
    publish is recoverable on the next poll.
    """
    runner_pk: UUID = runner.pk
    if purge_local:
        send_runner_remove(runner_pk, reason="deleted by user")
    else:
        send_runner_revoke(runner_pk, reason="deleted by user")
    # The session's ``revoked_reason`` is echoed in the 409
    # session_evicted body the daemon sees on its next poll. The Rust
    # synthesizer in ``runner/src/cloud/http.rs`` matches a small
    # canonical set of reasons and synthesizes a ``RemoveRunner`` frame
    # locally — which means **wipe the local install**.
    #
    # When ``purge_local=True`` we want that behaviour: send the
    # canonical ``runner_removed`` so the daemon falls back to local
    # cleanup if the wire frame above was lost in the enqueue-vs-evict
    # window. When ``purge_local=False`` the user explicitly asked us
    # NOT to touch the local install — pick a reason outside the
    # synthesizer's canonical set so the daemon exits its RunnerLoop
    # cleanly without wiping ``config.toml`` or the data dir.
    revoke_reason = "runner_removed" if purge_local else "user_revoke"
    with transaction.atomic():
        runner.revoke(reason=revoke_reason)
        close_runner_session(runner_pk)
        Runner.objects.filter(pk=runner_pk).delete()


def delete_dev_machine(machine: DevMachine, *, purge_local: bool) -> None:
    """Hard-delete a dev-machine *connection* cloud-side.

    This is the destructive counterpart to ``DevMachineRevokeEndpoint``:
    revoke leaves the machine (and its runner history) in the list marked
    revoked; delete removes the connection entirely. Because the
    ``DevMachine`` FKs are ``MachineSession=CASCADE``,
    ``MachineToken=SET_NULL`` and ``Runner=SET_NULL``, a bare
    ``machine.delete()`` would strand still-connecting runners and active
    tokens. So we tear them down first, then drop the row.

    Caller is responsible for authentication + authorization.

    Order mirrors ``delete_runner``: the daemon control frame for each
    runner is enqueued *before* ``runner.revoke()`` revokes the session —
    once the session is revoked ``enqueue_for_runner`` would divert the
    frame into the offline buffer that never drains for a row we are about
    to delete. So we enqueue for every active runner first, then revoke +
    close, then hard-delete the runner rows and the machine row (the
    latter cascades ``MachineSession``).

    ``purge_local`` matches ``delete_runner``: ``True`` sends
    ``remove_runner`` + the canonical ``runner_removed`` reason so the
    daemon also wipes the local ``[[runner]]`` block + data dir; ``False``
    sends a plain ``revoke`` with a non-canonical reason so the local
    install is left untouched.
    """
    machine_pk: UUID = machine.pk
    with transaction.atomic():
        now = timezone.now()
        MachineToken.objects.filter(dev_machine_id=machine_pk, revoked_at__isnull=True).update(revoked_at=now)
        runners = list(Runner.objects.select_for_update().filter(dev_machine_id=machine_pk))
        active = [r for r in runners if r.revoked_at is None]
        reason = "runner_removed" if purge_local else "user_revoke"
        for runner in active:
            if purge_local:
                send_runner_remove(runner.pk, reason="dev machine deleted")
            else:
                send_runner_revoke(runner.pk, reason="dev machine deleted")
        for runner in active:
            runner.revoke(reason=reason)
            close_runner_session(runner.pk)
        # Drop the runner rows explicitly (FK is SET_NULL, so a machine
        # delete alone would leave them orphaned) and then the machine row,
        # which cascades any MachineSession rows.
        Runner.objects.filter(dev_machine_id=machine_pk).delete()
        DevMachine.objects.filter(pk=machine_pk).delete()


def parse_purge_local(query_params, *, default: bool = True) -> bool:
    """Parse a ``purge_local`` query string flag with strict bool semantics.

    Treats ``"true"`` / ``"1"`` / ``"yes"`` (case-insensitive) as True,
    ``"false"`` / ``"0"`` / ``"no"`` as False, and an empty / missing
    value as ``default``. Raises ``ValueError`` on any other input so
    the caller can return HTTP 400.
    """
    raw = (query_params.get("purge_local") or "").strip().lower()
    if not raw:
        return default
    if raw in {"true", "1", "yes"}:
        return True
    if raw in {"false", "0", "no"}:
        return False
    raise ValueError(
        "purge_local must be one of: true, false, 1, 0, yes, no"
    )
