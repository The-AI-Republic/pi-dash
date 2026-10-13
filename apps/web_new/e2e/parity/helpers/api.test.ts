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
  analyticsAdvanceChartsRaw,
  analyticsAdvanceCustomChartsRaw,
  analyticsAdvanceStatsRaw,
  analyticsAdvanceTotalsRaw,
  analyticsProjectCreateStatus,
  analyticsProjectStateColors,
  analyticsProjectStatsRaw,
  analyticsScopedChartsRaw,
  analyticsScopedStatsRaw,
  analyticsScopedTotalsRaw,
  analyticsWorkspaceName,
  exportsFindRecord,
  exportsListHistory,
  exportsSubmitStatus,
  pagesLiveOracleBase,
  profileActivityExportCsv,
  serverPageRecord,
  serverPublishBoardFacts,
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

// Analytics overview/work-items chart reads (NEWFRONT-376): request shaping
// and response mapping against the stub, so the live specs only prove server
// behavior.
describe("analyticsAdvanceChartsRaw", () => {
  it("sends type without project_ids for the full workspace", async () => {
    stubRaw = JSON.stringify([{ key: "work_items", name: "Work Items", count: 2 }]);
    const read = await analyticsAdvanceChartsRaw(analyticsSession(), "anl-ws", "projects");
    expect(read.status).toBe(200);
    expect(read.payload).toEqual([{ key: "work_items", name: "Work Items", count: 2 }]);
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/advance-analytics-charts");
    expect(url.searchParams.get("type")).toBe("projects");
    expect(url.searchParams.has("project_ids")).toBe(false);
  });

  it("comma-joins the picked projects into project_ids", async () => {
    payload = { data: [], schema: {} };
    const read = await analyticsAdvanceChartsRaw(analyticsSession(), "anl-ws", "work-items", ["a", "b"]);
    expect(read.status).toBe(200);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.searchParams.get("type")).toBe("work-items");
    expect(url.searchParams.get("project_ids")).toBe("a,b");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsAdvanceChartsRaw(analyticsSession(), "anl-ws", "projects");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsProjectStatsRaw", () => {
  it("sends the list's fields query to the project-stats read", async () => {
    stubRaw = JSON.stringify([{ id: "p1", total_issues: 2, completed_issues: 1 }]);
    const read = await analyticsProjectStatsRaw(analyticsSession(), "anl-ws");
    expect(read.status).toBe(200);
    expect(read.payload).toEqual([{ id: "p1", total_issues: 2, completed_issues: 1 }]);
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/project-stats/");
    expect(url.searchParams.get("fields")).toBe("total_work_items,total_completed_work_items");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    const read = await analyticsProjectStatsRaw(analyticsSession(), "anl-ws");
    expect(read.status).toBe(500);
    expect(read.payload).toBeNull();
  });
});

// Workspace issue-exports oracle reads (NEWFRONT-381): request shaping and
// response mapping against the stub, so the live specs only prove server
// behavior. Plant/patch helpers shell into the stack container and are
// covered by the oracle scenarios that plant records, not here.
function exportsSession(): AuthedSession {
  return {
    email: "exp@example.com",
    password: "pw",
    userId: "user-1",
    cookie: "session-id=s; csrftoken=c",
    csrfToken: "c",
    apiBase,
  };
}

function exportRecord(id: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id,
    provider: "csv",
    status: "completed",
    url: "https://example.invalid/parity-exports/planted.csv",
    project: ["p1", "p2"],
    created_at: "2026-10-01T00:00:00Z",
    initiated_by_detail: {
      id: "user-1",
      display_name: "Exp Owner",
      email: "owner@example.com",
      avatar_url: null,
    },
    ...overrides,
  };
}

function historyPage(results: unknown, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    results,
    next_page_results: false,
    prev_page_results: false,
    next_cursor: "",
    prev_cursor: "",
    ...overrides,
  };
}

describe("exportsListHistory", () => {
  it("maps a history page with its cursors", async () => {
    payload = historyPage([exportRecord("e1")], {
      next_page_results: true,
      next_cursor: "10:1:1",
    });
    const page = await exportsListHistory(exportsSession(), "exp-ws");
    expect(page.results).toHaveLength(1);
    expect(page.results[0]).toEqual({
      id: "e1",
      provider: "csv",
      status: "completed",
      url: "https://example.invalid/parity-exports/planted.csv",
      project: ["p1", "p2"],
      created_at: "2026-10-01T00:00:00Z",
      initiated_by_detail: {
        id: "user-1",
        display_name: "Exp Owner",
        email: "owner@example.com",
        avatar_url: null,
      },
    });
    expect(page.next_page_results).toBe(true);
    expect(page.next_cursor).toBe("10:1:1");
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("GET");
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/exp-ws/export-issues/");
    expect(url.searchParams.get("cursor")).toBe("10:0:0");
    expect(url.searchParams.get("per_page")).toBe("10");
  });

  it("falls back to empty defaults for missing optional fields", async () => {
    payload = historyPage([{ id: "e2", initiated_by_detail: {} }]);
    const page = await exportsListHistory(exportsSession(), "exp-ws");
    expect(page.results[0]).toEqual({
      id: "e2",
      provider: "",
      status: "",
      url: null,
      project: [],
      created_at: "",
      initiated_by_detail: { id: "", display_name: "", email: "", avatar_url: null },
    });
  });

  it("reads an empty page when results carry no list", async () => {
    payload = historyPage(null);
    const page = await exportsListHistory(exportsSession(), "exp-ws");
    expect(page.results).toEqual([]);
  });

  it("throws when the history read fails", async () => {
    stubStatus = 500;
    await expect(exportsListHistory(exportsSession(), "exp-ws")).rejects.toThrow(
      "[parity] export history list failed with HTTP 500."
    );
  });

  it("rejects a record missing its id", async () => {
    payload = historyPage([{ provider: "csv" }]);
    await expect(exportsListHistory(exportsSession(), "exp-ws")).rejects.toThrow(
      "[parity] export record carried no id."
    );
  });
});

describe("exportsSubmitStatus", () => {
  it("posts provider, projects and the multiple flag with the status", async () => {
    stubStatus = 201;
    payload = { id: "e9" };
    const read = await exportsSubmitStatus(exportsSession(), "exp-ws", {
      provider: "csv",
      project: ["p1", "p2"],
      multiple: true,
    });
    expect(read.status).toBe(201);
    expect(read.payload).toEqual({ id: "e9" });
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("POST");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/exp-ws/export-issues/");
    expect(JSON.parse(stubSeen[0]?.body ?? "{}")).toEqual({
      provider: "csv",
      project: ["p1", "p2"],
      multiple: true,
    });
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    const read = await exportsSubmitStatus(exportsSession(), "exp-ws", {
      provider: "json",
      project: [],
      multiple: false,
    });
    expect(read.status).toBe(500);
    expect(read.payload).toBeNull();
  });
});

describe("exportsFindRecord", () => {
  it("finds a record on the first page", async () => {
    payload = historyPage([exportRecord("wanted-1"), exportRecord("other-2")]);
    const found = await exportsFindRecord(exportsSession(), "exp-ws", "wanted-1");
    expect(found?.id).toBe("wanted-1");
    expect(found?.provider).toBe("csv");
    expect(stubSeen).toHaveLength(1);
  });

  it("follows the cursor to a later page", async () => {
    let calls = 0;
    const first = historyPage([exportRecord("other-1")], {
      next_page_results: true,
      next_cursor: "10:1:1",
    });
    const second = historyPage([exportRecord("wanted-9")], { prev_page_results: true });
    payload = { toJSON: () => (calls++ === 0 ? first : second) } as unknown as Record<string, unknown>;
    const found = await exportsFindRecord(exportsSession(), "exp-ws", "wanted-9");
    expect(found?.id).toBe("wanted-9");
    expect(stubSeen).toHaveLength(2);
    const secondUrl = new URL(stubSeen[1]?.url ?? "", "http://stub");
    expect(secondUrl.searchParams.get("cursor")).toBe("10:1:1");
  });

  it("returns null when the record is absent and pages end", async () => {
    payload = historyPage([exportRecord("other-1")]);
    await expect(exportsFindRecord(exportsSession(), "exp-ws", "missing-7")).resolves.toBeNull();
  });
});

// Publish-board reads (NEWFRONT-390, PSET-016): the deploy-boards endpoint
// reports null/404 when nothing is published, so the helper maps every
// absent shape to null and only maps records carrying a string id.
describe("serverPublishBoardFacts", () => {
  it("returns null on 404 (never published or unpublished again)", async () => {
    stubStatus = 404;
    await expect(serverPublishBoardFacts("ws", "project-1", "session=x", apiBase)).resolves.toBeNull();
  });

  it("throws on a non-404 error response", async () => {
    stubStatus = 500;
    await expect(serverPublishBoardFacts("ws", "project-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] publish-board read failed with HTTP 500."
    );
  });

  it("returns null on an empty body", async () => {
    stubRaw = "";
    await expect(serverPublishBoardFacts("ws", "project-1", "session=x", apiBase)).resolves.toBeNull();
  });

  it("returns null on a JSON null body", async () => {
    stubRaw = "null";
    await expect(serverPublishBoardFacts("ws", "project-1", "session=x", apiBase)).resolves.toBeNull();
  });

  it("returns null when the record carries no string id", async () => {
    payload = { anchor: "abc" };
    await expect(serverPublishBoardFacts("ws", "project-1", "session=x", apiBase)).resolves.toBeNull();
  });

  it("maps a full published record through", async () => {
    payload = {
      id: "board-1",
      anchor: "live-anchor",
      is_comments_enabled: true,
      is_reactions_enabled: false,
      is_votes_enabled: true,
      view_props: { layout: "kanban" },
    };
    const facts = await serverPublishBoardFacts("ws", "project-1", "session=x", apiBase);
    expect(facts?.id).toBe("board-1");
    expect(facts?.anchor).toBe("live-anchor");
    expect(facts?.is_comments_enabled).toBe(true);
    expect(facts?.is_reactions_enabled).toBe(false);
    expect(facts?.is_votes_enabled).toBe(true);
    expect(facts?.view_props).toEqual({ layout: "kanban" });
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/projects/project-1/project-deploy-boards/");
  });

  it("defaults a missing anchor, non-true flags and non-object view props", async () => {
    payload = {
      id: "board-2",
      is_comments_enabled: 1,
      is_reactions_enabled: "true",
      view_props: "kanban",
    };
    const facts = await serverPublishBoardFacts("ws", "project-1", "session=x", apiBase);
    expect(facts?.anchor).toBeNull();
    expect(facts?.is_comments_enabled).toBe(false);
    expect(facts?.is_reactions_enabled).toBe(false);
    expect(facts?.is_votes_enabled).toBe(false);
    expect(facts?.view_props).toEqual({});
  });
});

describe("profileActivityExportCsv", () => {
  it("posts the date to the user activity export endpoint and returns the raw CSV text", async () => {
    stubStatus = 200;
    stubRaw = `"Actor name","Issue ID"\r\n"Ann","PX-1"\r\n`;
    const csv = await profileActivityExportCsv(exportsSession(), "exp-ws", "user-7", "2026-10-12");
    expect(csv).toBe(`"Actor name","Issue ID"\r\n"Ann","PX-1"\r\n`);
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("POST");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/exp-ws/user-activity/user-7/export/");
    expect(JSON.parse(stubSeen[0]?.body ?? "{}")).toEqual({ date: "2026-10-12" });
  });

  it("throws on a non-OK export response", async () => {
    stubStatus = 400;
    await expect(profileActivityExportCsv(exportsSession(), "exp-ws", "user-7", "")).rejects.toThrow(/HTTP 400/);
  });
});

// Analytics custom-insights / tables / scoped dialog reads (NEWFRONT-377):
// request shaping and response mapping against the stub, so the live specs
// only prove server behavior.
describe("analyticsAdvanceCustomChartsRaw", () => {
  it("sends the axis params without group_by for single-series", async () => {
    stubRaw = JSON.stringify({ data: [], schema: {} });
    const read = await analyticsAdvanceCustomChartsRaw(analyticsSession(), "anl-ws", {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
    });
    expect(read.status).toBe(200);
    expect(read.payload).toEqual({ data: [], schema: {} });
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/advance-analytics-charts");
    expect(url.searchParams.get("type")).toBe("custom-work-items");
    expect(url.searchParams.get("x_axis")).toBe("PRIORITY");
    expect(url.searchParams.get("y_axis")).toBe("WORK_ITEM_COUNT");
    expect(url.searchParams.has("group_by")).toBe(false);
    expect(url.searchParams.has("project_ids")).toBe(false);
  });

  it("carries group_by plus the picked projects when split and scoped", async () => {
    stubRaw = JSON.stringify({ data: [], schema: {} });
    const read = await analyticsAdvanceCustomChartsRaw(
      analyticsSession(),
      "anl-ws",
      { x_axis: "PRIORITY", y_axis: "WORK_ITEM_COUNT", group_by: "STATES" },
      ["a", "b"]
    );
    expect(read.status).toBe(200);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.searchParams.get("group_by")).toBe("STATES");
    expect(url.searchParams.get("project_ids")).toBe("a,b");
  });

  it("omits an empty group_by like a missing split", async () => {
    stubRaw = JSON.stringify({ data: [], schema: {} });
    const read = await analyticsAdvanceCustomChartsRaw(analyticsSession(), "anl-ws", {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
      group_by: "",
    });
    expect(read.status).toBe(200);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.searchParams.has("group_by")).toBe(false);
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsAdvanceCustomChartsRaw(analyticsSession(), "anl-ws", {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
    });
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsAdvanceStatsRaw", () => {
  it("sends the work-items stats type with the picked projects", async () => {
    stubRaw = JSON.stringify([{ project__name: "P" }]);
    const read = await analyticsAdvanceStatsRaw(analyticsSession(), "anl-ws", ["a"]);
    expect(read.status).toBe(200);
    expect(read.payload).toEqual([{ project__name: "P" }]);
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/advance-analytics-stats");
    expect(url.searchParams.get("type")).toBe("work-items");
    expect(url.searchParams.get("project_ids")).toBe("a");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsAdvanceStatsRaw(analyticsSession(), "anl-ws");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsScopedTotalsRaw", () => {
  it("reads totals through the project path with the scope params", async () => {
    stubRaw = JSON.stringify({ total_work_items: { count: 2 } });
    const read = await analyticsScopedTotalsRaw(analyticsSession(), "anl-ws", "p1", { cycle_id: "c9" });
    expect(read.status).toBe(200);
    expect(stubSeen).toHaveLength(1);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/projects/p1/advance-analytics");
    expect(url.searchParams.get("tab")).toBe("work-items");
    expect(url.searchParams.get("project_ids")).toBe("p1");
    expect(url.searchParams.get("cycle_id")).toBe("c9");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsScopedTotalsRaw(analyticsSession(), "anl-ws", "p1");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsScopedChartsRaw", () => {
  it("reads graphs through the project path with the graph type", async () => {
    stubRaw = JSON.stringify({ data: [], schema: {} });
    const read = await analyticsScopedChartsRaw(analyticsSession(), "anl-ws", "p1", "custom-work-items", {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
    });
    expect(read.status).toBe(200);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/projects/p1/advance-analytics-charts");
    expect(url.searchParams.get("type")).toBe("custom-work-items");
    expect(url.searchParams.get("x_axis")).toBe("PRIORITY");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsScopedChartsRaw(analyticsSession(), "anl-ws", "p1", "custom-work-items");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsScopedStatsRaw", () => {
  it("reads stats through the project path with the stats type", async () => {
    stubRaw = JSON.stringify([]);
    const read = await analyticsScopedStatsRaw(analyticsSession(), "anl-ws", "p1", { module_id: "m9" });
    expect(read.status).toBe(200);
    expect(read.payload).toEqual([]);
    const url = new URL(stubSeen[0]?.url ?? "", "http://stub");
    expect(url.pathname).toBe("/api/workspaces/anl-ws/projects/p1/advance-analytics-stats");
    expect(url.searchParams.get("type")).toBe("work-items");
    expect(url.searchParams.get("module_id")).toBe("m9");
  });

  it("carries the status with a null payload when the body is unparsable", async () => {
    stubStatus = 403;
    stubRaw = "forbidden";
    const read = await analyticsScopedStatsRaw(analyticsSession(), "anl-ws", "p1");
    expect(read.status).toBe(403);
    expect(read.payload).toBeNull();
  });
});

describe("analyticsProjectStateColors", () => {
  it("maps state rows to id, name, and color", async () => {
    stubRaw = JSON.stringify([
      { id: "s1", name: "Backlog", group: "backlog", color: "#60646c" },
      { id: "s2", name: "Done", group: "completed", color: "#22c55e" },
    ]);
    const rows = await analyticsProjectStateColors(analyticsSession(), "anl-ws", "p1");
    expect(rows).toEqual([
      { id: "s1", name: "Backlog", color: "#60646c" },
      { id: "s2", name: "Done", color: "#22c55e" },
    ]);
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.url).toBe("/api/workspaces/anl-ws/projects/p1/states/");
  });

  it("throws when a state row carries no color", async () => {
    stubRaw = JSON.stringify([{ id: "s1", name: "Backlog", group: "backlog" }]);
    await expect(analyticsProjectStateColors(analyticsSession(), "anl-ws", "p1")).rejects.toThrow(/no id\/name\/color/);
  });

  it("throws when the states read fails", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    await expect(analyticsProjectStateColors(analyticsSession(), "anl-ws", "p1")).rejects.toThrow(/HTTP 500/);
  });

  it("maps a non-list payload to an empty list", async () => {
    stubRaw = JSON.stringify({ data: [] });
    const rows = await analyticsProjectStateColors(analyticsSession(), "anl-ws", "p1");
    expect(rows).toEqual([]);
  });
});
