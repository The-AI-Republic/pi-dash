# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Canonical run-failure taxonomy — the single source of truth.

Every failed ``AgentRun`` stores one value from :class:`RunFailureReason`
in ``AgentRun.failure_reason``, classified at write time by
:func:`classify` from the runner's coarse ``FailureReason`` plus the raw
error text. The ticker consults :func:`policy_for` to decide whether a
retry can fix the failure (see ``orchestration/scheduling.py``), and the
read-time diagnostics (``runner/diagnostics.py``) derive their display
kinds from the stored value so display and policy cannot drift.

The string values are persisted and feed dashboards: they are a wire
contract. Renaming one is a breaking change — add new values instead.
Two groups, distinguished by prefix so callers never enumerate by hand:

- Platform side (no ``agent_error.`` prefix): the agent was not at fault
  and often never launched. These map directly from the runner protocol's
  ``FailureReason`` (``runner/src/cloud/protocol.rs``) or from cloud-side
  reapers.
- Agent side (``agent_error.*``): derived from the error text when the
  runner reports that the spawned agent itself failed.

Classifier design notes (see PDASHOSS01-183):

- HTTP status codes are matched only when not surrounded by other digits,
  so ``"402913 tokens"``, ``"15290ms"`` or ``"exit status 4030"`` never
  land in a provider bucket.
- Pre-launch failures (workspace setup, git auth, …) never reach the
  agent-text rules: the runner reason routes them platform-side by
  construction.
- Anthropic-compatible providers can use 403 for a concurrency rejection,
  so capacity wording is checked before the auth status codes.
- Misclassification fails safe: only the explicit
  :data:`~FailurePolicy.RETRY_FREE` allowlist is retried without spending
  budget; anything unrecognised falls into the normal-clock bucket.
- All regexes are compiled at import — this sits on the write path of
  every failed run.
"""

from __future__ import annotations

import re

from django.db import models


class RunFailureReason(models.TextChoices):
    """Stored failure reason for a failed ``AgentRun``.

    Values are a persisted wire contract (dashboards, API filters).
    """

    # -- Platform side: the agent was not at fault, often never launched --
    WORKSPACE_SETUP = "workspace_setup", "Workspace setup failed"
    GIT_AUTH = "git_auth", "Git authentication failed"
    #: Runner-side network failure (clone, cloud API) — transient.
    NETWORK = "network", "Runner network failure"
    #: The runner/daemon disappeared mid-run (heartbeat reap, desktop never
    #: connected). Distinct from ``daemon_restart``: nothing announced the
    #: outage.
    RUNNER_OFFLINE = "runner_offline", "Runner offline"
    DAEMON_RESTART = "daemon_restart", "Runner daemon restarted"
    #: The run was deliberately bounded and ran out of wall-clock (runner
    #: timeout, cloud hard limit, stall watchdog).
    TIMEOUT = "timeout", "Run timed out"
    MAX_TURNS = "max_turns", "Agent hit its turn budget"
    ASSIGN_REJECTED_BUSY = "assign_rejected_busy", "Runner busy, assignment rejected"
    INTERNAL = "internal", "Internal error"

    # -- Agent side: the spawned agent failed; derived from error text --
    AGENT_PROVIDER_AUTH = (
        "agent_error.provider_auth_or_access",
        "Provider authentication or access failed",
    )
    AGENT_PROVIDER_QUOTA = (
        "agent_error.provider_quota_limit",
        "Provider quota or credit limit",
    )
    AGENT_PROVIDER_CAPACITY = (
        "agent_error.provider_capacity_or_rate_limit",
        "Provider capacity or rate limit",
    )
    AGENT_PROVIDER_SERVER_ERROR = (
        "agent_error.provider_server_error",
        "Provider server error",
    )
    AGENT_PROVIDER_NETWORK = (
        "agent_error.provider_network",
        "Provider network failure",
    )
    AGENT_CONTEXT_OVERFLOW = (
        "agent_error.context_overflow",
        "Context window overflow",
    )
    AGENT_MODEL_NOT_FOUND = (
        "agent_error.model_not_found_or_unavailable",
        "Model not found or unavailable",
    )
    AGENT_MISSING_EXECUTABLE = (
        "agent_error.missing_executable",
        "Agent executable missing",
    )
    AGENT_MISSING_CONFIG = (
        "agent_error.missing_config",
        "Agent configuration missing",
    )
    AGENT_UNSUPPORTED_VERSION = (
        "agent_error.unsupported_version",
        "Unsupported agent version",
    )
    AGENT_EMPTY_OUTPUT = (
        "agent_error.empty_or_unparseable_output",
        "Empty or unparseable agent output",
    )
    AGENT_PROCESS_FAILURE = (
        "agent_error.process_failure",
        "Agent process failed",
    )
    AGENT_UNKNOWN = "agent_error.unknown", "Unclassified agent error"


#: Prefix that marks the agent-side group. Callers test the prefix, never
#: enumerate members by hand.
AGENT_ERROR_PREFIX = "agent_error."


def is_agent_side(reason: str) -> bool:
    return str(reason or "").startswith(AGENT_ERROR_PREFIX)


class FailurePolicy(models.TextChoices):
    """What the ticker should do about a failure with this reason."""

    #: Transient infrastructure: re-run on a short backoff without spending
    #: a tick, capped at :data:`FREE_RETRY_MAX_ATTEMPTS` consecutive tries.
    RETRY_FREE = "retry_free", "Retry soon, free"
    #: Maybe-transient: today's behaviour — keep ticking, spend a tick.
    RETRY_NORMAL = "retry_normal", "Retry on the normal clock"
    #: A retry will fail identically until a human acts: stop the clock,
    #: tell the user what to do, leave Re-tick available.
    NEEDS_HUMAN = "needs_human", "Needs a human"
    #: Retrying the same session reproduces the failure; the next run must
    #: start a fresh session. Ticks on the normal clock.
    FRESH_SESSION = "fresh_session", "Retry with a fresh session"


#: Consecutive same-reason free retries allowed before falling back to the
#: normal clock.
FREE_RETRY_MAX_ATTEMPTS = 3
#: Backoff before a free retry fires (short — the failure was transient).
FREE_RETRY_BACKOFF_SECONDS = 120
#: Backstop regardless of policy: this many runs in a row failing with the
#: same reason on one issue stops the clock for a human.
REPEATED_FAILURE_LIMIT = 3


_POLICY_BY_REASON: dict[str, str] = {
    # Retry soon, free — explicit allowlist; nothing falls in by default.
    RunFailureReason.NETWORK: FailurePolicy.RETRY_FREE,
    RunFailureReason.RUNNER_OFFLINE: FailurePolicy.RETRY_FREE,
    RunFailureReason.DAEMON_RESTART: FailurePolicy.RETRY_FREE,
    RunFailureReason.ASSIGN_REJECTED_BUSY: FailurePolicy.RETRY_FREE,
    RunFailureReason.AGENT_PROVIDER_NETWORK: FailurePolicy.RETRY_FREE,
    # Needs a human.
    RunFailureReason.WORKSPACE_SETUP: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.GIT_AUTH: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_PROVIDER_AUTH: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_PROVIDER_QUOTA: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_MODEL_NOT_FOUND: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_MISSING_EXECUTABLE: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_MISSING_CONFIG: FailurePolicy.NEEDS_HUMAN,
    RunFailureReason.AGENT_UNSUPPORTED_VERSION: FailurePolicy.NEEDS_HUMAN,
    # Fresh session.
    RunFailureReason.AGENT_CONTEXT_OVERFLOW: FailurePolicy.FRESH_SESSION,
}


def policy_for(reason: str) -> str:
    """Ticker policy for a stored reason. Unknown values fail safe onto the
    normal clock — never into the free-retry bucket."""
    return _POLICY_BY_REASON.get(str(reason or ""), FailurePolicy.RETRY_NORMAL)


# ---------------------------------------------------------------------------
# Classification
# ---------------------------------------------------------------------------

#: Runner-protocol ``FailureReason`` values that map 1:1 onto a platform
#: reason. ``codex_crash`` / ``agent_crash`` are absent on purpose: they
#: mean "the agent itself failed" and route to the text rules below. Older
#: runners only ever send values from this table plus the two crash kinds,
#: which is the backward-compatibility contract.
_RUNNER_REASON_MAP: dict[str, RunFailureReason] = {
    "workspace_setup": RunFailureReason.WORKSPACE_SETUP,
    "git_auth": RunFailureReason.GIT_AUTH,
    "network": RunFailureReason.NETWORK,
    "max_turns": RunFailureReason.MAX_TURNS,
    "timeout": RunFailureReason.TIMEOUT,
    "internal": RunFailureReason.INTERNAL,
    "daemon_restart": RunFailureReason.DAEMON_RESTART,
    "assign_rejected_busy": RunFailureReason.ASSIGN_REJECTED_BUSY,
    # Legacy value no longer emitted; a very old runner that still sends it
    # had a runner-side session problem, not an agent one.
    "resume_unavailable": RunFailureReason.INTERNAL,
}

_AGENT_CRASH_RUNNER_REASONS = frozenset({"agent_crash", "codex_crash"})

#: ``error_code`` values written by cloud-side FAILED writers (Cloud Agent
#: tasks, heartbeat reaper, managed-runner expiry). Consulted only when no
#: runner reason is available.
_ERROR_CODE_MAP: dict[str, RunFailureReason] = {
    "run_timeout": RunFailureReason.TIMEOUT,
    "dispatch_timeout": RunFailureReason.TIMEOUT,
    "heartbeat_reaped": RunFailureReason.RUNNER_OFFLINE,
    "desktop_not_connected": RunFailureReason.RUNNER_OFFLINE,
    "llm_config_missing": RunFailureReason.AGENT_MISSING_CONFIG,
    "gateway_scopes_missing": RunFailureReason.AGENT_MISSING_CONFIG,
    "byok_not_supported_on_desktop": RunFailureReason.AGENT_MISSING_CONFIG,
    "cloud_agent_disabled": RunFailureReason.INTERNAL,
    "managed_runner_disabled": RunFailureReason.INTERNAL,
    "actor_no_longer_authorized": RunFailureReason.INTERNAL,
    "prompt_too_large": RunFailureReason.AGENT_CONTEXT_OVERFLOW,
    "final_result_too_large": RunFailureReason.AGENT_EMPTY_OUTPUT,
}

#: Infrastructure failures recognisable from cloud-written detail strings
#: (prefix match, mirroring ``_INFRA_FAILURE_DETAIL_PREFIXES``). These are
#: platform-side by construction and must not reach the agent-text rules.
_INFRA_TEXT_PREFIXES: tuple[tuple[str, RunFailureReason], ...] = (
    ("daemon shutdown requested", RunFailureReason.DAEMON_RESTART),
    ("agent stalled: no events for >", RunFailureReason.TIMEOUT),
    # The runner's own stall watchdog: the detail embeds the last command's
    # stderr tail, which must not be fed to the agent-text rules (an npm
    # ENOTFOUND in the tail is not a provider network failure).
    ("no agent frames for", RunFailureReason.TIMEOUT),
    ("reaped by heartbeat", RunFailureReason.RUNNER_OFFLINE),
    ("cloud agent worker was lost", RunFailureReason.TIMEOUT),
    ("pi dash agent never came online", RunFailureReason.RUNNER_OFFLINE),
)


def _status(*codes: int) -> str:
    """Regex fragment matching HTTP status codes anchored on digit
    boundaries, so ``402913 tokens`` / ``15290ms`` / ``exit status 4030``
    never match a provider bucket."""
    return r"(?<![0-9])(?:" + "|".join(str(c) for c in codes) + r")(?![0-9])"


#: Ordered agent-side text rules — first match wins. Order is load-bearing:
#: context overflow before quota (its messages quote token *counts*),
#: capacity before auth (Anthropic-compatible providers use 403 for
#: concurrency rejections), model access before auth ("may not have
#: access"), and everything provider-ish before the generic process rules
#: (a provider error usually also mentions an exit status).
_AGENT_TEXT_RULES: tuple[tuple[RunFailureReason, re.Pattern[str]], ...] = tuple(
    (reason, re.compile(pattern, re.IGNORECASE))
    for reason, pattern in (
        (
            RunFailureReason.AGENT_CONTEXT_OVERFLOW,
            r"context[ _]window|context[ _]length|context_length_exceeded"
            r"|prompt is too long|maximum context|exceeds? the (?:model'?s )?context"
            r"|conversation (?:is )?too long|input length and `?max_tokens`? exceed",
        ),
        (
            RunFailureReason.AGENT_PROVIDER_CAPACITY,
            _status(429, 529)
            + r"|rate[ _-]?limit|too many requests|overloaded_error|\boverloaded\b"
            r"|concurrent connections|capacity constraint|server is busy"
            r"|number of request tokens has exceeded",
        ),
        (
            RunFailureReason.AGENT_PROVIDER_QUOTA,
            _status(402)
            + r"|insufficient (?:credit|quota|funds)|credit balance|out of credits"
            r"|quota exceeded|usage limit|billing|payment required"
            r"|\b(?:5-hour|weekly|monthly) limit\b|hard limit reached",
        ),
        (
            RunFailureReason.AGENT_MODEL_NOT_FOUND,
            r"selected model|model .{0,40}(?:not (?:found|exist|available)|may not exist)"
            r"|unknown model|model_not_found|no such model"
            r"|model is not supported|may not have access to the model",
        ),
        (
            RunFailureReason.AGENT_PROVIDER_AUTH,
            _status(401, 403, 407)
            + r"|authentication_failed|failed to authenticate|invalid authentication"
            r"|\bunauthorized\b|\bforbidden\b|invalid (?:api|access) key"
            r"|api key.{0,30}(?:invalid|expired|revoked|disabled)"
            r"|token (?:has )?expired|refresh(?:ing)? (?:the )?(?:oauth )?token"
            r"|re-?authenticate|please run /login|not logged in|login required"
            r"|invalid credentials|oauth",
        ),
        (
            RunFailureReason.AGENT_MISSING_EXECUTABLE,
            r"command not found|not recognized as an internal or external command"
            r"|(?:executable|program|binary) (?:file )?not found"
            r"|no such file or directory \(os error 2\)"
            r"|failed to (?:spawn|launch|exec)",
        ),
        (
            RunFailureReason.AGENT_MISSING_CONFIG,
            r"missing (?:api key|configuration|config)"
            r"|api key (?:is )?not (?:set|configured|found)|no api key"
            r"|config(?:uration)? (?:file )?(?:not found|missing)"
            r"|environment variable .{0,40}not set",
        ),
        (
            RunFailureReason.AGENT_UNSUPPORTED_VERSION,
            r"unsupported (?:\w+ )?version|version mismatch"
            r"|requires (?:at least )?version|please (?:upgrade|update) (?:to|your)",
        ),
        (
            RunFailureReason.AGENT_PROVIDER_SERVER_ERROR,
            _status(500, 502, 503, 504)
            + r"|internal server error|bad gateway|service unavailable"
            r"|gateway time-?out|\bapi_error\b|\bserver_error\b|upstream error",
        ),
        (
            RunFailureReason.AGENT_PROVIDER_NETWORK,
            r"connection (?:refused|reset|closed|aborted|error|timed out)"
            r"|econnreset|econnrefused|etimedout|enotfound|eai_again"
            r"|dns (?:error|failure|resolution)|could not resolve host"
            r"|network (?:error|is unreachable)|no route to host"
            r"|socket hang ?up|fetch failed|tls handshake|certificate (?:verify|error)",
        ),
        (
            RunFailureReason.AGENT_EMPTY_OUTPUT,
            r"done-signal parse error|empty (?:output|response|result)"
            r"|produced no (?:output|result)|no output received"
            r"|failed to parse|unexpected end of (?:json|input)",
        ),
        (
            RunFailureReason.AGENT_PROCESS_FAILURE,
            r"exit(?:ed)? (?:with )?(?:status|code)|non-?zero exit"
            r"|\bsig(?:kill|segv|term|abrt|bus)\b|terminated by signal"
            r"|panicked|core dumped|\bcrashed\b|process (?:died|exited|killed)",
        ),
    )
)


def consecutive_failure_streak(issue_id, reason: str) -> int:
    """How many of the issue's most recent runs failed with ``reason``.

    Counts from the newest run backwards and stops at the first run that is
    not FAILED-with-this-reason, so an interleaved success, cancel or
    different failure resets the streak. Conservative by construction: a
    non-terminal replacement run also breaks it.
    """
    from pi_dash.runner.models import AgentRun, AgentRunStatus

    window = REPEATED_FAILURE_LIMIT + FREE_RETRY_MAX_ATTEMPTS + 2
    rows = (
        AgentRun.objects.filter(work_item_id=issue_id)
        .order_by("-created_at")
        .values_list("status", "failure_reason")[:window]
    )
    streak = 0
    for status, stored in rows:
        if status != AgentRunStatus.FAILED or (stored or "") != reason:
            break
        streak += 1
    return streak


def repeated_failure_limit_for(reason: str) -> int:
    """The consecutive-same-reason streak at which the backstop stops the
    clock. Free-retry reasons get their retry allowance first, so their
    backstop sits just past :data:`FREE_RETRY_MAX_ATTEMPTS`."""
    if policy_for(reason) == FailurePolicy.RETRY_FREE:
        return max(REPEATED_FAILURE_LIMIT, FREE_RETRY_MAX_ATTEMPTS + 1)
    return REPEATED_FAILURE_LIMIT


def _classify_agent_text(detail: str) -> RunFailureReason:
    for reason, pattern in _AGENT_TEXT_RULES:
        if pattern.search(detail):
            return reason
    return RunFailureReason.AGENT_UNKNOWN


def classify(
    raw_error: str,
    *,
    runner_reason: str = "",
    error_code: str = "",
) -> RunFailureReason:
    """Canonical failure reason for a failed run.

    ``runner_reason`` is the runner protocol's ``FailureReason`` string when
    the failure arrived over the runner wire; ``error_code`` is the
    cloud-side code for server-written failures (Cloud Agent, reapers).
    Platform-side reasons win by construction — the agent-text rules only
    run when the runner says the agent itself crashed, or when nothing
    structured is known about the failure.
    """
    reason_key = str(runner_reason or "").strip().lower()
    if reason_key in _RUNNER_REASON_MAP:
        return _RUNNER_REASON_MAP[reason_key]

    detail = str(raw_error or "").strip()
    if reason_key in _AGENT_CRASH_RUNNER_REASONS:
        return _classify_agent_text(detail)

    code_key = str(error_code or "").strip().lower()
    if code_key in _ERROR_CODE_MAP:
        return _ERROR_CODE_MAP[code_key]

    lowered = detail.lower()
    for prefix, reason in _INFRA_TEXT_PREFIXES:
        if lowered.startswith(prefix):
            return reason

    return _classify_agent_text(detail)


__all__ = [
    "AGENT_ERROR_PREFIX",
    "FREE_RETRY_BACKOFF_SECONDS",
    "FREE_RETRY_MAX_ATTEMPTS",
    "REPEATED_FAILURE_LIMIT",
    "FailurePolicy",
    "RunFailureReason",
    "classify",
    "consecutive_failure_streak",
    "is_agent_side",
    "policy_for",
    "repeated_failure_limit_for",
]
