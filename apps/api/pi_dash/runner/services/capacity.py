# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dev-machine run-queue capacity contract helpers.

``.ai_design/dev_machine_run_queue/implementation.md`` §3 is normative
for the wire/storage/recovery details implemented here. This module
is the Django half of T1+T2: attempt identity (token minting,
protocol selection, legacy adoption rules), capacity-post validation,
the shared Runner → active RunnerSession → AgentRun lock order,
freshness checks for the reaper guards, and the poll capacity
snapshot application.

Kept import-light (models + stdlib only) so matcher, views, tasks
and session service can all use it without cycles.
"""

from __future__ import annotations

import logging
import uuid

from django.conf import settings
from django.db import connection
from django.utils import timezone

from pi_dash.runner.models import (
    MACHINE_RUN_QUEUE_CAPABILITY,
    AssignmentProtocol,
    RunnerSession,
)

logger = logging.getLogger(__name__)

# Re-exported so call sites read one module for the contract.
PROTOCOL_LEGACY = int(AssignmentProtocol.LEGACY)
PROTOCOL_MACHINE_QUEUE = int(AssignmentProtocol.MACHINE_QUEUE)
CAPABILITY_MACHINE_QUEUE = MACHINE_RUN_QUEUE_CAPABILITY

# Capacity POST wire limits (§3): 16 KiB body, u64 revisions, 1–65535
# limit, 90s server-receipt freshness.
CAPACITY_BODY_LIMIT_BYTES = 16 * 1024
REVISION_MAX = 2**64 - 1
CAPACITY_FRESH_SECS = 90
CAPACITY_MIN_LIMIT = 1
CAPACITY_MAX_LIMIT = 65535

PHASE_WAITING = "waiting_for_capacity"
PHASE_PREPARING = "preparing"

# Capacity endpoint actions.
CAPACITY_ACTIONS = ("waiting", "admit", "defer", "reconcile")


def runner_supports_machine_queue(runner) -> bool:
    """True when the runner's current active session advertises v1.

    Reads through the runner's active :class:`RunnerSession` (no lock;
    callers hold the matcher's assignment transaction, which keeps the
    Runner → Session order). Missing session or missing capability
    means legacy (protocol 0).
    """
    if runner is None:
        return False
    caps = (
        RunnerSession.objects.filter(runner=runner, revoked_at__isnull=True)
        .values_list("capabilities", flat=True)
        .first()
    )
    if not isinstance(caps, list):
        return False
    return CAPABILITY_MACHINE_QUEUE in caps


def protocol_for_runner(runner) -> int:
    """Assignment protocol for a fresh assignment to ``runner``."""
    return PROTOCOL_MACHINE_QUEUE if runner_supports_machine_queue(runner) else PROTOCOL_LEGACY


def mint_assignment_attempt(run, runner):
    """Stamp fresh attempt identity on ``run``; the caller persists it.

    Called inside every new assignment transaction (pod drain,
    dedicated/direct assignment, reassign — including to the same
    runner): a fresh UUIDv4 token is minted unconditionally, the
    protocol follows the assigning runner's negotiated capability, and
    any prior capacity presentation is cleared. Session redelivery
    never calls this (it preserves the attempt).
    """
    run.assignment_token = uuid.uuid4()
    run.assignment_protocol = protocol_for_runner(runner)
    run.capacity_wait = None
    return run


def parse_revision(value):
    """u64 revision or ``None`` when invalid.

    Mandatory for capacity mutations; rejects bools, floats
    (fractional), negatives, overflow and non-ints.
    """
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    if value < 0 or value > REVISION_MAX:
        return None
    return value


def parse_uuid(value):
    """UUID for a valid UUID string, else ``None``."""
    try:
        return uuid.UUID(str(value))
    except (ValueError, AttributeError, TypeError):
        return None


def token_matches(run, provided) -> bool:
    """Exact attempt-token equality (``provided`` is a UUID or None).

    A run with no stored token only matches a tokenless report (legacy
    rows carry null). A stored token never matches a tokenless report.
    """
    stored = getattr(run, "assignment_token", None)
    if stored is None:
        return provided is None
    return provided is not None and uuid.UUID(str(stored)) == provided


def fresh_capacity_wait(run, now=None):
    """The stored capacity_wait when it is a fresh same-attempt wait.

    Fresh means: phase ``waiting_for_capacity``, the payload's token
    matches the run's current attempt token, and ``reported_at`` is
    within :data:`CAPACITY_FRESH_SECS` of server receipt (falling back
    to "treat as fresh" only when ``reported_at`` is absent for
    back-compat — every store path writes it). Returns the dict or
    ``None``. The reaper guards exclude these runs from silence/stall
    failures; preparing never qualifies.
    """
    cw = getattr(run, "capacity_wait", None)
    if not isinstance(cw, dict) or cw.get("phase") != PHASE_WAITING:
        return None
    if parse_uuid(cw.get("assignment_token")) is None:
        return None
    stored_token = getattr(run, "assignment_token", None)
    if stored_token is None or str(stored_token) != str(cw.get("assignment_token")):
        return None
    reported_raw = cw.get("reported_at")
    if reported_raw is None:
        return cw
    try:
        from datetime import datetime

        reported = datetime.fromisoformat(str(reported_raw).replace("Z", "+00:00"))
    except (ValueError, AttributeError):
        return None
    now = now or timezone.now()
    if reported.tzinfo is None:
        from datetime import timezone as _tz

        reported = reported.replace(tzinfo=_tz.utc)
    if (now - reported).total_seconds() > CAPACITY_FRESH_SECS:
        return None
    return cw


def bound_txn_waits() -> None:
    """Cap lock + statement waits for the CURRENT transaction (Postgres only).

    Same convoy guard as the session endpoints: a runner-scoped row
    lock with no timeout parks the worker until the proxy 504s. On
    timeout Postgres raises ``OperationalError``; callers translate
    it to a 503 so the daemon backs off instead of feeding the
    convoy. ``SET LOCAL`` scopes the caps to the enclosing
    transaction.
    """
    if connection.vendor != "postgresql":
        return
    lock_ms = int(getattr(settings, "RUNNER_TXN_LOCK_TIMEOUT_MS", 5000))
    stmt_ms = int(getattr(settings, "RUNNER_TXN_STATEMENT_TIMEOUT_MS", 20000))
    with connection.cursor() as cursor:
        cursor.execute(f"SET LOCAL lock_timeout = '{lock_ms}ms'")
        cursor.execute(f"SET LOCAL statement_timeout = '{stmt_ms}ms'")


def parse_session_capabilities(body: dict) -> list:
    """Daemon-advertised session capabilities (list of strings).

    Additive and defaulted: a missing/non-list value yields ``[]`` so
    older daemons keep protocol 0. Entries are trimmed to strings and
    capped in count/length so a malformed payload cannot bloat the row.
    """
    raw = (body or {}).get("capabilities")
    if not isinstance(raw, list):
        return []
    caps = [str(item)[:128] for item in raw if isinstance(item, str)][:32]
    return caps


def validate_count(value, *, allow_zero: bool, positive: bool = False):
    """Capacity count field or ``None`` when invalid."""
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    if positive:
        return value if value >= 1 else None
    if allow_zero:
        return value if value >= 0 else None
    return value if value > 0 else None
