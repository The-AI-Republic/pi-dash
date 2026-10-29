# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The fail-open MCP toolset wrapper, isolated from module-load-time imports.

This lives apart from ``runtime.mcp`` on purpose. ``ResilientToolset`` subclasses
pydantic-ai's ``WrapperToolset``, and a ``class`` statement cannot defer its base
class — so keeping the class in ``mcp.py`` would force ``import pydantic_ai`` at
that module's import time. ``mcp.py`` is reached at Django URLconf load time, so
that would drag pydantic-ai (and its transitive tree) into *every* Django
process — the web server, ``migrate``, every management command — not just the
ones that run an assistant turn. Isolating the class here lets ``mcp.py`` import
it lazily inside ``build_toolset``, matching how the rest of the assistant
runtime (``runtime/llm.py``, ``tasks.py``) defers pydantic-ai to function scope.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field

from pydantic_ai import exceptions as pydantic_ai_exceptions
from pydantic_ai.toolsets import WrapperToolset

logger = logging.getLogger(__name__)

# pydantic-ai exceptions that mean "handle this", not "the server broke".
# Mirrors what ``pydantic_ai.tool_manager`` re-raises rather than converting,
# and is resolved by name so a version that renames or drops one degrades to
# treating it as a server failure instead of failing at import.
_CONTROL_FLOW_EXCEPTIONS: tuple[type[BaseException], ...] = tuple(
    t
    for t in (
        getattr(pydantic_ai_exceptions, name, None)
        for name in ("ModelRetry", "ToolRetryError", "SkipToolExecution", "CallDeferred", "ApprovalRequired")
    )
    if isinstance(t, type) and issubclass(t, BaseException)
)


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
