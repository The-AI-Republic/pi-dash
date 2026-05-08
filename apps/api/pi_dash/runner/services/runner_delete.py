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

from pi_dash.runner.models import Runner
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
    """
    runner_pk: UUID = runner.pk
    if purge_local:
        send_runner_remove(runner_pk, reason="deleted by user")
    else:
        send_runner_revoke(runner_pk, reason="deleted by user")
    # Use the canonical ``runner_removed`` revoke reason so the
    # session-evicted body the daemon sees (sessions.py emits
    # ``session.revoked_reason`` in the 409 body) carries a reason
    # the daemon can match against to fall back to local cleanup if
    # the ``remove_runner`` frame above was lost in the small window
    # between enqueue and session eviction. See
    # ``runner/src/cloud/http.rs`` for the synthesizer.
    runner.revoke(reason="runner_removed")
    close_runner_session(runner_pk)
    Runner.objects.filter(pk=runner_pk).delete()


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
