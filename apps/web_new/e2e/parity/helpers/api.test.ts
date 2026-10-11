// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for serverPageRecord in helpers/api.ts (NEWFRONT-320): a page
// nobody has touched yet carries no string updated_by, so the reader maps
// it to null instead of throwing; pages with a recorded updater keep their
// string mapping. The HTTP layer is a stub server, so these never touch a
// real stack.
import { createServer, type Server } from "node:http";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { pagesLiveOracleBase, serverPageRecord } from "./api";

let payload: Record<string, unknown>;
let server: Server;
let apiBase: string;

beforeEach(async () => {
  payload = {};
  server = createServer((_req, res) => {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify(payload));
  });
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const address = server.address();
  if (typeof address !== "object" || address === null) throw new Error("[parity] stub server bound no port.");
  apiBase = `http://127.0.0.1:${String(address.port)}`;
});

afterEach(async () => {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error === undefined) resolve();
      else reject(error);
    });
  });
});

function freshPage(): Record<string, unknown> {
  return {
    id: "page-1",
    name: "Untitled",
    access: 0,
    owned_by: "owner-1",
    updated_by: null,
    created_at: "2026-10-11T00:00:00Z",
    updated_at: "2026-10-11T00:00:00Z",
    description_html: "<p></p>",
    is_locked: false,
  };
}

describe("serverPageRecord", () => {
  it("maps a fresh page's null updated_by to null instead of throwing", async () => {
    payload = freshPage();
    const record = await serverPageRecord("ws", "project", "page-1", "session=x", apiBase);
    expect(record.id).toBe("page-1");
    expect(record.updatedBy).toBeNull();
    expect(record.ownedBy).toBe("owner-1");
    expect(record.isLocked).toBe(false);
  });

  it("maps a recorded updater and lock through", async () => {
    payload = { ...freshPage(), updated_by: "editor-2", is_locked: true };
    const record = await serverPageRecord("ws", "project", "page-1", "session=x", apiBase);
    expect(record.updatedBy).toBe("editor-2");
    expect(record.isLocked).toBe(true);
  });

  it("still rejects a record missing a required string key", async () => {
    payload = freshPage();
    delete payload["id"];
    await expect(serverPageRecord("ws", "project", "page-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] page record carried no string id."
    );
  });
});

// Oracle-base wrappers must derive from PARITY_NS like every other suite
// URL (NEWFRONT-353): a hardcoded :13000 default points at the wrong stack
// under a bare namespaced run.
describe("pagesLiveOracleBase", () => {
  const names = ["PARITY_NS", "PARITY_ORACLE_URL", "PARITY_ORACLE_PORT"] as const;
  let saved: Record<string, string | undefined>;

  beforeEach(() => {
    saved = {};
    for (const name of names) {
      saved[name] = process.env[name];
      delete process.env[name];
    }
  });

  afterEach(() => {
    for (const name of names) {
      const value = saved[name];
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
  });

  it("derives the oracle port from PARITY_NS", () => {
    process.env["PARITY_NS"] = "257";
    expect(pagesLiveOracleBase()).toBe("http://localhost:13057");
  });

  it("keeps the legacy default with no namespace", () => {
    expect(pagesLiveOracleBase()).toBe("http://localhost:13000");
  });

  it("lets an explicit PARITY_ORACLE_URL win, trimmed of trailing slashes", () => {
    process.env["PARITY_NS"] = "257";
    process.env["PARITY_ORACLE_URL"] = "http://proxy:8080//";
    expect(pagesLiveOracleBase()).toBe("http://proxy:8080");
  });
});
