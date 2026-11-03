// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry contracts against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { createMcpServer, deleteMcpServer, listMcpServers, patchMcpServer } from "./assistant-registry.js";

const SERVER = "123e4567-e89b-12d3-a456-426614174000";
const AT = "2026-10-16T15:32:05.479661+09:00";

const ROW = {
  id: SERVER,
  name: "Jira",
  url: "https://mcp.example.com",
  has_auth_header: true,
  tool_prefix: "mcp_jira",
  effective_tool_prefix: "mcp_jira",
  is_enabled: true,
  created_at: AT,
  updated_at: AT,
};

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  post?: ApiClient["post"];
  delete?: ApiClient["delete"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

const NO_CONTENT = {
  status: 204,
  url: "https://api.test/x/",
  headers: { get: () => null },
  text: async () => "",
};

describe("registry", () => {
  it("lists the servers with resolved prefixes", async () => {
    const calls: string[] = [];
    const client = stubClient({
      getJson: (async (url: string) => {
        calls.push(url);
        return [ROW, { ...ROW, is_enabled: false, effective_tool_prefix: null }];
      }) as ApiClient["getJson"],
    });
    const rows = await listMcpServers(client);
    expect(calls).toEqual(["/api/users/me/ai-assistant/mcp-servers/"]);
    expect(rows).toHaveLength(2);
    expect(rows[1]?.effective_tool_prefix).toBeNull();
  });

  it("creates a server and parses the echo", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { ...ROW, effective_tool_prefix: null }, status: 201, url };
      }) as ApiClient["sendJson"],
    });
    const row = await createMcpServer(client, {
      name: "Jira",
      url: "https://mcp.example.com",
      auth_header: "Bearer secret",
    });
    expect(calls[0]?.url).toBe("/api/users/me/ai-assistant/mcp-servers/");
    expect(calls[0]?.json).toEqual({
      name: "Jira",
      url: "https://mcp.example.com",
      auth_header: "Bearer secret",
      is_enabled: undefined,
    });
    expect(row.effective_tool_prefix).toBeNull();
  });

  it("patches a server through the detail route", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { ...ROW, name: "Jira v2" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const row = await patchMcpServer(client, SERVER, { name: "Jira v2", is_enabled: false });
    expect(calls[0]?.url).toBe(`/api/users/me/ai-assistant/mcp-servers/${SERVER}/`);
    expect(calls[0]?.json).toEqual({ name: "Jira v2", url: undefined, auth_header: undefined, is_enabled: false });
    expect(row.name).toBe("Jira v2");
  });

  it("deletes through the detail route", async () => {
    const calls: string[] = [];
    const client = stubClient({
      delete: (async (url: string) => {
        calls.push(url);
        return NO_CONTENT;
      }) as ApiClient["delete"],
    });
    await expect(deleteMcpServer(client, SERVER)).resolves.toBeUndefined();
    expect(calls).toEqual([`/api/users/me/ai-assistant/mcp-servers/${SERVER}/`]);
  });
});
