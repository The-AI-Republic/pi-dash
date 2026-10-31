// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: tool-server registry CRUD against live Django. Every
// test removes the rows it creates; names carry a timestamp so
// re-runs never collide on the per-user uniqueness.
import { describe, expect, it } from "vitest";
import { ApiClient } from "../client.js";
import { assistantErrorFrom } from "../contracts/assistant-errors.js";
import { createMcpServer, deleteMcpServer, listMcpServers, patchMcpServer } from "../contracts/assistant-registry.js";
import { isApiError } from "../errors.js";
import { contractEnabled, sharedSignedIn } from "./setup.js";

async function cleanServers(client: ApiClient): Promise<void> {
  const rows = await listMcpServers(client);
  for (const row of rows) await deleteMcpServer(client, row.id);
}

describe.skipIf(!contractEnabled)("assistant registry contract", () => {
  it("round-trips a server through create, read, patch and delete", async () => {
    const client = await sharedSignedIn();
    const name = `Contract MCP ${Date.now()}`;
    const created = await createMcpServer(client, {
      name,
      url: "https://mcp.example.com",
      auth_header: "Bearer contract-secret",
    });
    try {
      expect(created.has_auth_header).toBe(true);
      expect(created.effective_tool_prefix).toBeNull();
      const rows = await listMcpServers(client);
      const mine = rows.find((row) => row.id === created.id);
      expect(mine?.tool_prefix).toBe(created.tool_prefix);
      expect(mine?.effective_tool_prefix).toBe(created.tool_prefix);
      const renamed = await patchMcpServer(client, created.id, { name: `${name} v2`, is_enabled: false });
      expect(renamed.name).toBe(`${name} v2`);
      expect(renamed.is_enabled).toBe(false);
      const relisted = await listMcpServers(client);
      expect(relisted.find((row) => row.id === created.id)?.effective_tool_prefix).toBeNull();
      const duplicate: unknown = await createMcpServer(client, {
        name: `${name} v2`,
        url: "https://mcp.example.com",
      }).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(duplicate)).toMatchObject({ code: "duplicate_name", status: 400 });
    } finally {
      await deleteMcpServer(client, created.id);
    }
    const gone: unknown = await deleteMcpServer(client, created.id).then(
      () => null,
      (error: unknown) => error
    );
    expect(isApiError(gone) && gone.status).toBe(404);
    const patched: unknown = await patchMcpServer(client, created.id, { name: "ghost" }).then(
      () => null,
      (error: unknown) => error
    );
    expect(isApiError(patched) && patched.status).toBe(404);
  });

  it("disambiguates colliding slugs with a counter", async () => {
    const client = await sharedSignedIn();
    const stamp = Date.now();
    const first = await createMcpServer(client, {
      name: `Probe Tools ${stamp}`,
      url: "https://mcp-one.example.com",
    });
    const second = await createMcpServer(client, {
      name: `probe-tools-${stamp}`,
      url: "https://mcp-two.example.com",
    });
    try {
      expect(first.tool_prefix).toBe(second.tool_prefix);
      const rows = await listMcpServers(client);
      const prefixes = rows
        .filter((row) => row.id === first.id || row.id === second.id)
        .map((row) => row.effective_tool_prefix)
        .sort();
      expect(prefixes).toEqual([first.tool_prefix, `${first.tool_prefix}_2`].sort());
    } finally {
      await deleteMcpServer(client, first.id);
      await deleteMcpServer(client, second.id);
    }
  });

  it("refuses past the per-user server ceiling", async () => {
    const client = await sharedSignedIn();
    await cleanServers(client);
    const ids: string[] = [];
    try {
      for (let index = 0; index < 10; index += 1) {
        const row = await createMcpServer(client, {
          name: `Contract ceiling ${Date.now()}-${index}`,
          url: `https://mcp-${index}.example.com`,
        });
        ids.push(row.id);
      }
      const over: unknown = await createMcpServer(client, {
        name: `Contract ceiling ${Date.now()}-over`,
        url: "https://mcp-over.example.com",
      }).then(
        () => null,
        (error: unknown) => error
      );
      expect(assistantErrorFrom(over)).toMatchObject({ code: "too_many_servers", status: 400 });
    } finally {
      for (const id of ids) await deleteMcpServer(client, id);
    }
  });
});
