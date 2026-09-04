# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""MCP tool servers for the assistant runtime.

Turns ``AssistantMCPServer`` rows into pydantic-ai toolsets. The agent enters
each toolset for the duration of a run (pydantic-ai manages that via its
internal ``AsyncExitStack``), so a server's tools are discovered per turn and
nothing is cached across turns — a server the user just added is usable on the
next message, and one they removed is gone immediately.

**Failure policy: fail open.** A tool server that is unreachable, slow, or
misbehaving must never take the assistant down with it; the turn proceeds with
whatever toolsets did build. Callers surface the skipped servers to the user
(see ``build_toolsets``' second return value).
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field

from django.conf import settings

from pydantic_ai import exceptions as pydantic_ai_exceptions
from pydantic_ai.toolsets import WrapperToolset

from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import AssistantMCPServer

logger = logging.getLogger(__name__)

# Connect timeout. pydantic-ai defaults to 5s, which is tight for a cold
# server but fine as a connect bound.
DEFAULT_TIMEOUT_S = 10.0
# Read timeout for a single MCP call, matching pydantic-ai's own default.
# Tool calls commonly traverse a third upstream (the tool's own backend) and
# long-running tools — search, build, anything LLM-backed — routinely need
# minutes, so this must stay generous: anything tighter turns a slow tool into
# a failed one.
DEFAULT_READ_TIMEOUT_S = 300.0
# Ceiling on enabled servers per user. Toolsets are entered sequentially at the
# start of every turn, each bounded only by the connect timeout, so the count is
# what bounds the dead time a user can inflict on their own turns: N dead
# servers cost N x DEFAULT_TIMEOUT_S before the model is even called.
DEFAULT_MAX_SERVERS = 10


#: pydantic-ai exceptions that mean "handle this", not "the server broke".
#: Mirrors what ``pydantic_ai.tool_manager`` re-raises rather than converting,
#: and is resolved by name so a version that renames or drops one degrades to
#: treating it as a server failure instead of failing at import.
_CONTROL_FLOW_EXCEPTIONS: tuple[type[BaseException], ...] = tuple(
    t
    for t in (
        getattr(pydantic_ai_exceptions, name, None)
        for name in ("ModelRetry", "ToolRetryError", "SkipToolExecution", "CallDeferred", "ApprovalRequired")
    )
    if isinstance(t, type) and issubclass(t, BaseException)
)


@dataclass(frozen=True)
class SkippedServer:
    """A server that could not be turned into a toolset, and why."""

    name: str
    reason: str


@dataclass
class ResilientToolset(WrapperToolset):
    """Wraps a toolset so a failing server degrades instead of failing the turn.

    Building an MCP toolset performs no I/O — the connection is opened when the
    agent *enters* it, at the start of the run. Without this wrapper a server
    that is down raises out of ``Agent.run`` and takes the whole turn with it,
    so one broken tool server would cost the user their assistant entirely.

    All three points where a server can reach out are covered: connecting,
    listing tools, and *calling* one. The last matters most in practice — a
    server that connects fine at turn start can still time out or drop mid-run,
    and pydantic-ai's tool manager only converts ``ModelRetry``/``ToolError``,
    so anything else propagates out of ``Agent.run``.

    ``failure`` records what happened so the caller can tell the user which
    server was dropped rather than leaving them to wonder why a capability
    silently vanished.
    """

    #: Every field carries a default on purpose. pydantic-ai rebuilds wrappers
    #: with ``dataclasses.replace(self, wrapped=...)`` in ``for_run``,
    #: ``for_run_step`` and ``visit_and_replace``, which reconstructs through
    #: ``__init__`` passing only the dataclass fields. A required argument here
    #: — or a hand-written ``__init__`` that adds one — turns every such rebuild
    #: into a TypeError, and it would fire exactly where this wrapper exists to
    #: prevent a hard failure. Dormant today because ``MCPToolset.for_run``
    #: returns ``self``; a pydantic-ai upgrade is all it takes to wake it.
    server_name: str = ""
    #: The tool prefix assigned to this server for the run. Carried on the
    #: outermost wrapper because that is the object callers hold — the
    #: prefixing wrapper underneath doesn't surface it.
    prefix: str = ""
    #: Run state, not configuration: a rebuilt wrapper starts clean rather than
    #: inheriting a failure recorded against a connection it no longer holds.
    failure: str | None = field(default=None, init=False, compare=False)
    _entered: bool = field(default=False, init=False, compare=False, repr=False)

    def _record(self, exc: Exception, what: str) -> None:
        self.failure = type(exc).__name__
        logger.warning(
            "mcp server %s, continuing without it: %s (%s)",
            what,
            self.server_name,
            exc,
        )

    async def __aenter__(self):
        try:
            await super().__aenter__()
            self._entered = True
        except Exception as exc:  # noqa: BLE001 — a dead server is not a turn failure
            self._record(exc, "unreachable")
        return self

    async def __aexit__(self, *args) -> bool | None:
        if not self._entered:
            # Never entered, so there is nothing to unwind — and calling the
            # wrapped __aexit__ would raise on a half-built connection.
            return None
        try:
            return await super().__aexit__(*args)
        except Exception as exc:  # noqa: BLE001 — teardown is still server I/O
            # A transport can disappear after the final tool call but before
            # the session's close handshake completes. That is the same
            # additive-server outage as a connect/list/call failure: record it
            # for the user, but do not replace the assistant turn's outcome
            # with an MCP cleanup exception.
            self._record(exc, "failed to close")
            return None

    async def get_tools(self, ctx):
        if self.failure is not None:
            return {}
        try:
            return await super().get_tools(ctx)
        except Exception as exc:  # noqa: BLE001 — same rule as connect
            self._record(exc, "failed to list tools")
            return {}

    async def call_tool(self, name, tool_args, ctx, tool):
        """Absorb a mid-run tool failure into the tool's own result.

        The server was reachable when the run started or this tool would not be
        on offer, so a failure here is the server dying, timing out, or erroring
        mid-turn. Returning the failure as the tool's result keeps the turn
        alive and lets the model react to it; raising would end the turn, and
        ``ModelRetry`` would burn the run's retries on a server that is not
        coming back.

        pydantic-ai's own control-flow exceptions pass through untouched: they
        are decisions, not outages, and the tool manager is what acts on them.
        """
        try:
            return await super().call_tool(name, tool_args, ctx, tool)
        except _CONTROL_FLOW_EXCEPTIONS:
            raise
        except Exception as exc:  # noqa: BLE001 — a dying server is not a turn failure
            self._record(exc, f"failed calling {name}")
            return f"Tool server {self.server_name!r} was unavailable for this call ({type(exc).__name__})."


def build_toolset(
    *,
    url: str,
    auth_header: str | None = None,
    tool_prefix: str | None = None,
    include_instructions: bool = False,
    timeout: float = DEFAULT_TIMEOUT_S,
    read_timeout: float = DEFAULT_READ_TIMEOUT_S,
    server_name: str = "",
):
    """Build one streamable-HTTP MCP toolset.

    ``include_instructions`` forwards the server's ``instructions`` to the
    model. It defaults to False in pydantic-ai; a server that uses
    instructions to describe a discovery protocol needs it on or the model
    never learns the protocol exists.

    Uses ``MCPToolset`` rather than the deprecated ``MCPServerStreamableHTTP``
    (removed in pydantic-ai v2); streamable HTTP is its default for http URLs.
    """
    from pydantic_ai.mcp import MCPToolset

    headers = {"Authorization": auth_header} if auth_header else None
    toolset = MCPToolset(
        url,
        headers=headers,
        include_instructions=include_instructions,
        init_timeout=timeout,
        read_timeout=read_timeout,
    )
    # Prefixing is a wrapper in the current API rather than a constructor arg.
    if tool_prefix:
        toolset = toolset.prefixed(tool_prefix)
    # Outermost, so it also absorbs failures raised by the prefix wrapper.
    return ResilientToolset(toolset, server_name=server_name or url, prefix=tool_prefix or "")


def unique_prefixes(servers: list[AssistantMCPServer]) -> dict:
    """Map each server to a tool prefix unique within this run.

    Names are unique per user, but slugification is lossy — "My Tools",
    "my-tools" and "my_tools" all reduce to ``my_tools``. Two servers sharing a
    prefix would expose colliding tool names to the model, which silently
    shadows one server's tools with another's. Disambiguate with a counter,
    ordered by ``created_at`` so a given server's prefix is stable as long as
    the ones before it are unchanged.
    """
    used: set[str] = set()
    assigned: dict = {}
    for server in servers:
        base = server.tool_prefix
        prefix = base
        n = 2
        while prefix in used:
            prefix = f"{base}_{n}"
            n += 1
        used.add(prefix)
        assigned[server.pk] = prefix
    return assigned


def _auth_header_for(server: AssistantMCPServer) -> str | None:
    if not server.has_auth_header:
        return None
    return crypto.decrypt(server.auth_header_encrypted)


def build_toolsets(user) -> tuple[list, list[SkippedServer]]:
    """Return ``(toolsets, skipped)`` for ``user``'s enabled MCP servers.

    Never raises for a per-server problem: a blocked URL or an undecryptable
    auth header yields a :class:`SkippedServer` entry instead, so one bad row
    cannot break every turn the user takes.
    """
    limit = max_servers()
    servers = list(AssistantMCPServer.objects.filter(user=user, is_enabled=True))
    prefixes = unique_prefixes(servers)

    toolsets: list = []
    skipped: list[SkippedServer] = []

    # Defence in depth behind the create-time limit: rows can predate the cap
    # or outlive a lowered one, and every enabled row costs connect time on
    # every turn. Report the overflow rather than dropping it silently — the
    # user is owed the reason their newest server never runs. Ordering is the
    # model's ``created_at``, so which servers survive is stable.
    if len(servers) > limit:
        for server in servers[limit:]:
            skipped.append(SkippedServer(server.name, "too_many_servers"))
        servers = servers[:limit]

    for server in servers:
        if ssrf.is_blocked(server.url):
            skipped.append(SkippedServer(server.name, "url_blocked"))
            continue
        try:
            auth_header = _auth_header_for(server)
        except AssistantError as exc:
            # Crypto not configured, or ciphertext from a retired key.
            skipped.append(SkippedServer(server.name, exc.code))
            continue
        except Exception:  # noqa: BLE001 — a broken row must not break the turn
            logger.exception("mcp server auth header decrypt failed: %s", server.id)
            skipped.append(SkippedServer(server.name, "auth_header_unreadable"))
            continue

        try:
            toolsets.append(
                build_toolset(
                    url=server.url,
                    auth_header=auth_header,
                    tool_prefix=prefixes[server.pk],
                    timeout=_timeout_setting(),
                    read_timeout=_read_timeout_setting(),
                    server_name=server.name,
                )
            )
        except Exception:  # noqa: BLE001 — construction should not fail, but never fatal
            logger.exception("mcp toolset construction failed: %s", server.id)
            skipped.append(SkippedServer(server.name, "toolset_unavailable"))

    return toolsets, skipped


def _timeout_setting() -> float:
    return float(getattr(settings, "ASSISTANT_MCP_TIMEOUT_S", DEFAULT_TIMEOUT_S))


def _read_timeout_setting() -> float:
    return float(getattr(settings, "ASSISTANT_MCP_READ_TIMEOUT_S", DEFAULT_READ_TIMEOUT_S))


def max_servers() -> int:
    """Ceiling on enabled servers per user, enforced on create and at build."""
    return int(getattr(settings, "ASSISTANT_MCP_MAX_SERVERS", DEFAULT_MAX_SERVERS))
