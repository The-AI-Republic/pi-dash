/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { PlugZap } from "lucide-react";
import type { IAssistantSkippedServer } from "@pi-dash/types";

// Machine-readable reason codes the backend emits for a dropped tool server,
// mapped to short human text. The set covers OSS (mcp.py + the resolver
// total-failure branch), the surfaced crypto AssistantError code, and the
// cloud overlay's OpenHub reason. Runtime mid-run failures report a raw Python
// exception class name rather than a fixed code, so anything unmapped falls
// back to the raw reason below.
const REASON_TEXT: Record<string, string> = {
  url_blocked: "its URL is not allowed",
  auth_header_unreadable: "its credential could not be read",
  toolset_unavailable: "it could not be reached",
  toolsets_unavailable: "tools were unavailable",
  assistant_not_configured: "its credential could not be decrypted",
  openhub_unavailable: "OpenHub apps were unavailable",
};

function reasonText(reason: string): string {
  return REASON_TEXT[reason] ?? reason;
}

function noticeLine(server: IAssistantSkippedServer): string {
  const detail = reasonText(server.reason);
  // The resolver's total-failure sentinel isn't a real server name; phrase it
  // as a whole-capability outage rather than "Tool server all tool servers…".
  if (server.name === "all tool servers") {
    return `Tool servers were unavailable for this reply (${detail}).`;
  }
  return `Tool server ${server.name} was unavailable for this reply (${detail}).`;
}

/**
 * Unobtrusive inline notice for a `tool_servers_skipped` event. Rendered in the
 * thread (not a toast) because the event can recur every turn while a server is
 * down.
 */
export function ToolServersSkippedNotice({ servers }: { servers: IAssistantSkippedServer[] }) {
  if (servers.length === 0) return null;
  return (
    <div className="flex items-start gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-12 text-secondary">
      <PlugZap className="mt-0.5 size-3.5 shrink-0" />
      <div className="flex flex-col gap-0.5">
        {servers.map((s) => (
          <span key={`${s.name}:${s.reason}`}>{noticeLine(s)}</span>
        ))}
      </div>
    </div>
  );
}
