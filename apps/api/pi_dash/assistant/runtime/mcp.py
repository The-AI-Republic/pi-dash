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
from dataclasses import dataclass

from django.conf import settings

from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import AssistantMCPServer

logger = logging.getLogger(__name__)

# Connect timeout. pydantic-ai defaults to 5s, which is tight for a cold
# server but fine as a connect bound.
DEFAULT_TIMEOUT_S = 10.0
# Read timeout for a single MCP call. Tool calls commonly traverse a third
# upstream (the tool's own backend), so this is deliberately generous relative
# to the connect bound.
DEFAULT_READ_TIMEOUT_S = 60.0


@dataclass(frozen=True)
class SkippedServer:
    """A server that could not be turned into a toolset, and why."""

    name: str
    reason: str


def build_toolset(
    *,
    url: str,
    auth_header: str | None = None,
    tool_prefix: str | None = None,
    include_instructions: bool = False,
    timeout: float = DEFAULT_TIMEOUT_S,
    read_timeout: float = DEFAULT_READ_TIMEOUT_S,
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
    return toolset.prefixed(tool_prefix) if tool_prefix else toolset


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
    toolsets: list = []
    skipped: list[SkippedServer] = []

    for server in AssistantMCPServer.objects.filter(user=user, is_enabled=True):
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
                    tool_prefix=server.tool_prefix,
                    timeout=_timeout_setting(),
                    read_timeout=_read_timeout_setting(),
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
