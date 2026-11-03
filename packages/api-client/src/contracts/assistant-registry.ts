// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry: the per-user MCP servers a turn may call.
// Managed in settings, never in chat — chat only reports the ones a
// run had to skip.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, uuid } from "./fields.js";

/**
 * One registry row. The auth header is write-only and never echoes —
 * only its presence. `tool_prefix` is the row's own slug;
 * `effective_tool_prefix` is the prefix a run actually assigns (a
 * counter disambiguates slug collisions), null for a disabled row,
 * which claims no prefix. The create and update echoes carry null
 * there — only the list resolves it.
 */
export const McpServer = z.object({
  id: uuid(),
  name: z.string(),
  url: z.string(),
  has_auth_header: z.boolean(),
  tool_prefix: z.string(),
  effective_tool_prefix: z.nullable(z.string()),
  is_enabled: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type McpServer = z.infer<typeof McpServer>;

export const McpServerList = z.array(McpServer);
export type McpServerList = z.infer<typeof McpServerList>;

export interface McpServerInput {
  name: string;
  url: string;
  /**
   * Full Authorization header value. Write-only: send "" to clear a
   * stored one, omit to leave it untouched.
   */
  auth_header?: string;
  is_enabled?: boolean;
}

/** List the user's tool servers, oldest first, prefixes resolved. */
export async function listMcpServers(client: ApiClient): Promise<McpServerList> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/mcp-servers/");
  return client.parse(McpServerList, body, "GET /api/users/me/ai-assistant/mcp-servers/");
}

/**
 * Register a tool server; answers 201. Refusals (duplicate name,
 * blocked host, over the per-user ceiling) reject with the
 * assistant failure shape.
 */
export async function createMcpServer(client: ApiClient, input: McpServerInput): Promise<McpServer> {
  const path = "/api/users/me/ai-assistant/mcp-servers/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      name: input.name,
      url: input.url,
      auth_header: input.auth_header,
      is_enabled: input.is_enabled,
    },
  });
  return client.parse(McpServer, envelope.data, `POST ${path}`);
}

export interface McpServerPatch {
  name?: string;
  url?: string;
  auth_header?: string;
  is_enabled?: boolean;
}

/** Rename, repoint, re-credential, or toggle a tool server. */
export async function patchMcpServer(client: ApiClient, serverId: string, patch: McpServerPatch): Promise<McpServer> {
  const path = `/api/users/me/ai-assistant/mcp-servers/${serverId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, {
    json: {
      name: patch.name,
      url: patch.url,
      auth_header: patch.auth_header,
      is_enabled: patch.is_enabled,
    },
  });
  return client.parse(McpServer, envelope.data, `PATCH ${path}`);
}

/** Remove a tool server; resolves on the 204. */
export async function deleteMcpServer(client: ApiClient, serverId: string): Promise<void> {
  await client.delete(`/api/users/me/ai-assistant/mcp-servers/${serverId}/`);
}
