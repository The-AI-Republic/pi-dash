"""Per-run in-process FastMCP adapter for the closed GitHub read catalog."""

from __future__ import annotations

import asyncio

from asgiref.sync import sync_to_async
from django.conf import settings

GITHUB_TOOL_NAMES = frozenset({"github_get_file", "github_get_linked_pull_request"})


def build_github_mcp(run_id, allowed_names):
    """Build an isolated server that closes over one verified AgentRun ID."""
    from fastmcp import FastMCP
    from pi_dash.cloud_agent.tools import build_tools

    granted = sorted(set(allowed_names) & GITHUB_TOOL_NAMES)
    sync_tools = {tool.__name__: tool for tool in build_tools(run_id, granted, source="mcp", server_key="github")}
    server = FastMCP(
        name=f"pi-dash-github-{run_id}",
        instructions=None,
        tasks=False,
        mask_error_details=True,
        strict_input_validation=True,
    )

    if "github_get_file" in sync_tools:

        @server.tool(
            name="github_get_file",
            annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True},
            run_in_thread=False,
            timeout=settings.CLOUD_AGENT_TOOL_TIMEOUT_SECONDS,
        )
        async def github_get_file(path: str, ref: str = ""):
            """Read a bounded UTF-8 file from the run's verified project repository."""
            return await sync_to_async(sync_tools["github_get_file"], thread_sensitive=True)(path, ref)

    if "github_get_linked_pull_request" in sync_tools:

        @server.tool(
            name="github_get_linked_pull_request",
            annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True},
            run_in_thread=False,
            timeout=settings.CLOUD_AGENT_TOOL_TIMEOUT_SECONDS,
        )
        async def github_get_linked_pull_request(aspect: str = "summary"):
            """Read one bounded aspect of the GitHub PR already linked to the run's issue."""
            return await sync_to_async(sync_tools["github_get_linked_pull_request"], thread_sensitive=True)(aspect)

    return server


def build_github_toolset(run_id, allowed_names):
    from pydantic_ai.mcp import MCPToolset
    from pi_dash.cloud_agent.tools import current_tool_call_id

    async def process_tool_call(ctx, call_tool, name, args):
        token = current_tool_call_id.set(ctx.tool_call_id)
        try:
            async with asyncio.timeout(settings.CLOUD_AGENT_TOOL_TIMEOUT_SECONDS):
                return await call_tool(name, args)
        finally:
            current_tool_call_id.reset(token)

    server = build_github_mcp(run_id, allowed_names)
    return MCPToolset(
        server,
        id=f"github-{run_id}",
        include_instructions=False,
        process_tool_call=process_tool_call,
        cache_resources=False,
        cache_prompts=False,
        tool_error_behavior="error",
        read_timeout=settings.CLOUD_AGENT_TOOL_TIMEOUT_SECONDS,
    )
