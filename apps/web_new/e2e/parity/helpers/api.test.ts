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
import {
  NETWORK,
  analyticsAdvanceTotalsRaw,
  analyticsProjectCreateStatus,
  analyticsWorkspaceName,
  pagesLiveOracleBase,
  serverPageRecord,
  type AuthedSession,
} from "./api";

let payload: Record<string, unknown>;
let server: Server;
let apiBase: string;
/** Override the stub's status (analytics tests); defaults to the old 200. */
let stubStatus: number;
/** Serve this raw body instead of JSON(payload) (analytics tests). */
let stubRaw: string | null;
/** Requests the stub has served since the last reset (analytics tests). */
let stubSeen: Array<{ method: string; url: string; body: string }>;

beforeEach(async () => {
  payload = {};
  stubStatus = 200;
  stubRaw = null;
  stubSeen = [];
  server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk: Buffer) => {
      body += chunk.toString("utf8");
    });
    req.on("end", () => {
      stubSeen.push({ method: req.method ?? "", url: req.url ?? "", body });
      res.statusCode = stubStatus;
      res.setHeader("content-type", "application/json");
      res.end(stubRaw ?? JSON.stringify(payload));
    });
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

// Analytics shell oracle reads (NEWFRONT-375): request shaping and response
// mapping against the stub, so the live specs only prove server behavior.
function analyticsSession(): AuthedSession {
  return {
    email: "anl@example.com",
    password: "pw",
    userId: "user-1",
    cookie: "session-id=s; csrftoken=c",
    csrfToken: "c",
    apiBase,
  };
}

describe("analyticsWorkspaceName", () => {
  it("reads the workspace display name", async () => {
    payload = { name: "Anl WS" };
    await expect(analyticsWorkspaceName(analyticsSession(), "anl-ws")).resolves.toBe("Anl WS");
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("GET");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/anl-ws/");
  });

  it("throws when the workspace carries no string name", async () => {
    payload = {};
    await expect(analyticsWorkspaceName(analyticsSession(), "anl-ws")).rejects.toThrow(
      "[parity] workspace read carried no string name."
    );
  });

  it("throws when the workspace read fails", async () => {
    stubStatus = 404;
    await expect(analyticsWorkspaceName(analyticsSession(), "anl-ws")).rejects.toThrow(
      "[parity] workspace read failed with HTTP 404."
    );
  });
});

describe("analyticsAdvanceTotalsRaw", () => {
  it("sends tab without project_ids for the full workspace", async () => {
    payload = { total_projects: { count: 2 } };
    const read = await analyticsAdvanceTotalsRaw(analyticsSession(), "anl-ws", "overview");
    expect(read.status).toBe(200);
    expect(read.payload).toEqual({ total_projects: { count: 2 } });
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/advance-analytics");
    expect(url.searchParams.get("tab")).toBe("overview");
    expect(url.searchParams.has("project_ids")).toBe(false);
  });

  it("comma-joins the picked projects into project_ids", async () => {
    payload = { total_projects: { count: 1 } };
    const read = await analyticsAdvanceTotalsRaw(analyticsSession(), "anl-ws", "work-items", ["a", "b"]);
    expect(read.status).toBe(200);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.searchParams.get("tab")).toBe("work-items");
    expect(url.searchParams.get("project_ids")).toBe("a,b");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsAdvanceTotalsRaw(analyticsSession(), "anl-ws", "overview");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsProjectCreateStatus", () => {
  it("posts the project form and returns the status", async () => {
    stubStatus = 201;
    payload = { id: "p1" };
    const status = await analyticsProjectCreateStatus(analyticsSession(), "anl-ws", {
      name: "Anl Made",
      identifier: "ANL0101",
    });
    expect(status).toBe(201);
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("POST");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/anl-ws/projects/");
    expect(JSON.parse(stubSeen[0]?.body ?? "{}")).toEqual({
      name: "Anl Made",
      identifier: "ANL0101",
      network: NETWORK.PUBLIC,
    });
  });

  it("returns refusals instead of throwing", async () => {
    stubStatus = 403;
    const status = await analyticsProjectCreateStatus(analyticsSession(), "anl-ws", {
      name: "Guest Try",
      identifier: "GT0101",
    });
    expect(status).toBe(403);
  });
});
