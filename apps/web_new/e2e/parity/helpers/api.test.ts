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
  browserSessionCookies,
  exportsFindRecord,
  exportsListHistory,
  exportsSubmitStatus,
  pagesLiveOracleBase,
  profileActivityExportCsv,
  serverApiTokens,
  serverCreateApiToken,
  serverBindRepositoryRaw,
  serverDeleteProjectRaw,
  serverIdentifierTaken,
  serverPageRecord,
  serverProjectFetchStatus,
  serverProjectRecord,
  serverPublishBoardFacts,
  serverRepositoryStatus,
  serverSearchIssueNames,
  serverWorkspacePods,
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

// Personal-access-token list + full-field create (NEWFRONT-413,
// WSET-025/026/027/028): request shaping and row mapping against the
// stub, so the live specs only prove server behavior. The list maps
// every row through userApiTokenOf; the create posts only the fields
// the caller provided and resolves with the one-time secret.
describe("serverApiTokens", () => {
  it("maps rows through with defaults for missing optional fields", async () => {
    stubRaw = JSON.stringify([
      {
        id: "tok-1",
        label: "ci-token",
        description: "runs the suite",
        expired_at: "2030-06-15T12:00:00Z",
        is_active: true,
      },
      { id: "tok-2", label: "bare-token" },
    ]);
    const rows = await serverApiTokens("session=x", apiBase);
    expect(rows).toEqual([
      {
        id: "tok-1",
        label: "ci-token",
        description: "runs the suite",
        expiredAt: "2030-06-15T12:00:00Z",
        active: true,
        hasToken: false,
      },
      {
        id: "tok-2",
        label: "bare-token",
        description: "",
        expiredAt: null,
        active: false,
        hasToken: false,
      },
    ]);
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("GET");
    expect(stubSeen[0]?.url).toBe("/api/users/api-tokens/");
  });

  it("flags a row carrying the raw secret and a null expiry", async () => {
    stubRaw = JSON.stringify([
      {
        id: "tok-3",
        label: "fresh-token",
        description: "",
        expired_at: null,
        is_active: true,
        token: "pd_raw_secret",
      },
    ]);
    const rows = await serverApiTokens("session=x", apiBase);
    expect(rows).toEqual([
      {
        id: "tok-3",
        label: "fresh-token",
        description: "",
        expiredAt: null,
        active: true,
        hasToken: true,
      },
    ]);
  });

  it("throws when a row carries no string id/label", async () => {
    stubRaw = JSON.stringify([{ id: "tok-4", label: 42 }]);
    await expect(serverApiTokens("session=x", apiBase)).rejects.toThrow(
      "[parity] api-token row carried no string id/label."
    );
  });

  it("throws when the list read fails", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    await expect(serverApiTokens("session=x", apiBase)).rejects.toThrow(/HTTP 500/);
  });
});

describe("serverCreateApiToken", () => {
  it("posts a label-only body and resolves with the id plus secret", async () => {
    payload = { id: "tok-5", label: "label-only", token: "pd_secret_5" };
    const created = await serverCreateApiToken("session=x", { label: "label-only" }, apiBase);
    expect(created).toEqual({ id: "tok-5", token: "pd_secret_5" });
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("POST");
    expect(stubSeen[0]?.url).toBe("/api/users/api-tokens/");
    expect(JSON.parse(stubSeen[0]?.body ?? "")).toEqual({ label: "label-only" });
  });

  it("includes description and expiry only when provided", async () => {
    payload = { id: "tok-6", label: "full", token: "pd_secret_6" };
    const created = await serverCreateApiToken(
      "session=x",
      { label: "full", description: "d", expiredAt: "2030-01-01T00:00:00Z" },
      apiBase
    );
    expect(created).toEqual({ id: "tok-6", token: "pd_secret_6" });
    expect(JSON.parse(stubSeen[0]?.body ?? "")).toEqual({
      label: "full",
      description: "d",
      expired_at: "2030-01-01T00:00:00Z",
    });
  });

  it("throws when the create response carries no id/token", async () => {
    payload = { id: "tok-7", label: "no-secret" };
    await expect(serverCreateApiToken("session=x", { label: "no-secret" }, apiBase)).rejects.toThrow(
      "[parity] api-token create response carried no id/token."
    );
  });

  it("throws when the create fails", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    await expect(serverCreateApiToken("session=x", { label: "x" }, apiBase)).rejects.toThrow(/HTTP 500/);
  });
});

// Project settings General tab readers (NEWFRONT-389, PSET-005–012): the
// same stub-server shape as the board-facts tests above.
describe("serverProjectRecord", () => {
  it("returns the raw project record on OK", async () => {
    payload = { id: "project-1", name: "Atlas", identifier: "ATL" };
    const record = await serverProjectRecord("ws", "project-1", "session=x", apiBase);
    expect(record).toEqual({ id: "project-1", name: "Atlas", identifier: "ATL" });
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/projects/project-1/");
  });

  it("throws on a non-OK project read", async () => {
    stubStatus = 404;
    await expect(serverProjectRecord("ws", "project-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] project read failed with HTTP 404."
    );
  });
});

describe("serverProjectFetchStatus", () => {
  it("returns the status with a null record on a refusal", async () => {
    stubStatus = 403;
    await expect(serverProjectFetchStatus("ws", "project-1", "session=x", apiBase)).resolves.toEqual({
      status: 403,
      record: null,
    });
  });

  it("returns the status with the record on OK", async () => {
    payload = { id: "project-1", name: "Atlas" };
    const fetched = await serverProjectFetchStatus("ws", "project-1", "session=x", apiBase);
    expect(fetched.status).toBe(200);
    expect(fetched.record).toEqual({ id: "project-1", name: "Atlas" });
  });
});

describe("serverDeleteProjectRaw", () => {
  it("resolves with the status and body on success", async () => {
    stubRaw = "";
    const outcome = await serverDeleteProjectRaw("ws", "project-1", "session=x", apiBase);
    expect(outcome.status).toBe(200);
    expect(stubSeen[0]?.method).toBe("DELETE");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/projects/project-1/");
  });

  it("resolves with the refusal body instead of throwing", async () => {
    stubStatus = 403;
    stubRaw = JSON.stringify({ error: "default project" });
    const outcome = await serverDeleteProjectRaw("ws", "project-1", "session=x", apiBase);
    expect(outcome.status).toBe(403);
    expect(outcome.body).toContain("default project");
  });
});

describe("serverIdentifierTaken", () => {
  it("returns true when the reservation count is positive", async () => {
    payload = { exists: 2 };
    await expect(serverIdentifierTaken("ws", "ATL", "session=x", apiBase)).resolves.toBe(true);
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/project-identifiers?name=ATL");
  });

  it("returns false on a zero count or a missing key", async () => {
    payload = { exists: 0 };
    await expect(serverIdentifierTaken("ws", "NEW", "session=x", apiBase)).resolves.toBe(false);
    payload = {};
    await expect(serverIdentifierTaken("ws", "NEW", "session=x", apiBase)).resolves.toBe(false);
  });

  it("throws on a non-OK identifier lookup", async () => {
    stubStatus = 500;
    await expect(serverIdentifierTaken("ws", "ATL", "session=x", apiBase)).rejects.toThrow(
      "[parity] identifier lookup failed with HTTP 500."
    );
  });
});

describe("serverRepositoryStatus", () => {
  it("returns the raw repository record on OK", async () => {
    payload = { repo_url: "https://github.com/acme/atlas", synced: true };
    const record = await serverRepositoryStatus("ws", "project-1", "session=x", apiBase);
    expect(record).toEqual({ repo_url: "https://github.com/acme/atlas", synced: true });
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/projects/project-1/repository/");
  });

  it("throws on a non-OK repository read", async () => {
    stubStatus = 404;
    await expect(serverRepositoryStatus("ws", "project-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] repository read failed with HTTP 404."
    );
  });
});

describe("serverBindRepositoryRaw", () => {
  it("posts the repo URL and resolves with the raw outcome", async () => {
    stubStatus = 401;
    stubRaw = JSON.stringify({ error: "GitHub token is missing" });
    const outcome = await serverBindRepositoryRaw(
      "ws",
      "project-1",
      "https://github.com/acme/atlas",
      "session=x",
      apiBase
    );
    expect(outcome.status).toBe(401);
    expect(outcome.body).toContain("GitHub token is missing");
    expect(stubSeen[0]?.method).toBe("POST");
    expect(stubSeen[0]?.url).toBe("/api/workspaces/ws/projects/project-1/repository/bind/");
    expect(JSON.parse(stubSeen[0]?.body ?? "{}")).toEqual({ repo_url: "https://github.com/acme/atlas" });
  });
});

describe("serverSearchIssueNames", () => {
  it("returns the issue names for an array payload", async () => {
    stubRaw = JSON.stringify([{ name: "Alpha" }, { name: "Beta" }]);
    const names = await serverSearchIssueNames("ws", "project-1", "alp", "session=x", apiBase);
    expect(names).toEqual(["Alpha", "Beta"]);
    expect(stubSeen[0]?.url).toBe(
      "/api/workspaces/ws/projects/project-1/search-issues/?search=alp&workspace_search=false"
    );
  });

  it("returns an empty list for a non-array payload and drops non-string names", async () => {
    stubRaw = JSON.stringify({ results: [] });
    await expect(serverSearchIssueNames("ws", "project-1", "alp", "session=x", apiBase)).resolves.toEqual([]);
    stubRaw = JSON.stringify([{ name: "Alpha" }, { name: 7 }, {}]);
    await expect(serverSearchIssueNames("ws", "project-1", "alp", "session=x", apiBase)).resolves.toEqual(["Alpha"]);
  });

  it("throws on a non-OK issue search", async () => {
    stubStatus = 500;
    await expect(serverSearchIssueNames("ws", "project-1", "alp", "session=x", apiBase)).rejects.toThrow(
      "[parity] issue search failed with HTTP 500."
    );
  });
});

describe("serverWorkspacePods", () => {
  it("lists the workspace pods and maps rows with defaults", async () => {
    stubRaw = JSON.stringify([
      {
        id: "pod-1",
        name: "PAR_pod_1",
        description: "seed pod",
        is_default: true,
        project: "project-1",
        project_identifier: "PAR",
        runner_count: 5,
      },
      { id: "pod-2", name: "ARC701429_pod_1" },
    ]);
    const rows = await serverWorkspacePods("ws-1", "session=x", apiBase);
    expect(rows).toEqual([
      {
        id: "pod-1",
        name: "PAR_pod_1",
        description: "seed pod",
        isDefault: true,
        project: "project-1",
        projectIdentifier: "PAR",
        runnerCount: 5,
      },
      {
        id: "pod-2",
        name: "ARC701429_pod_1",
        description: "",
        isDefault: false,
        project: "",
        projectIdentifier: "",
        runnerCount: 0,
      },
    ]);
    expect(stubSeen).toHaveLength(1);
    expect(stubSeen[0]?.method).toBe("GET");
    expect(stubSeen[0]?.url).toBe("/api/runners/pods/?workspace=ws-1");
  });

  it("throws when a row carries no string id/name", async () => {
    stubRaw = JSON.stringify([{ id: "pod-3", name: 42 }]);
    await expect(serverWorkspacePods("ws-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] pod row carried no string id/name."
    );
  });

  it("throws when the list read fails", async () => {
    stubStatus = 500;
    stubRaw = "boom";
    await expect(serverWorkspacePods("ws-1", "session=x", apiBase)).rejects.toThrow(
      "[parity] pod list failed with HTTP 500."
    );
  });

  it("defaults the API base from the environment", async () => {
    stubRaw = JSON.stringify([{ id: "pod-4", name: "PAR_pod_1" }]);
    const prior = process.env.PARITY_API_URL;
    process.env.PARITY_API_URL = apiBase;
    try {
      const rows = await serverWorkspacePods("ws-1", "session=x");
      expect(rows.map((row) => row.name)).toEqual(["PAR_pod_1"]);
      expect(stubSeen).toHaveLength(1);
      expect(stubSeen[0]?.url).toBe("/api/runners/pods/?workspace=ws-1");
    } finally {
      if (prior === undefined) delete process.env.PARITY_API_URL;
      else process.env.PARITY_API_URL = prior;
    }
  });
});

describe("browserSessionCookies", () => {
  const prior = process.env.PARITY_ORACLE_URL;

  beforeEach(() => {
    process.env.PARITY_ORACLE_URL = "http://oracle.test:13000";
  });

  afterEach(() => {
    if (prior === undefined) delete process.env.PARITY_ORACLE_URL;
    else process.env.PARITY_ORACLE_URL = prior;
  });

  it("reads the session and CSRF cookies out of a signed-in session", () => {
    const cookies = browserSessionCookies({
      email: "a@example.com",
      password: "pw",
      userId: "user-1",
      cookie: "session-id=abc123; csrftoken=csrf456",
      csrfToken: "csrf456",
      apiBase: "http://oracle.test:13000",
    });
    expect(cookies).toEqual([
      { name: "session-id", value: "abc123", domain: "oracle.test", path: "/", httpOnly: true, sameSite: "Lax" },
      { name: "csrftoken", value: "csrf456", domain: "oracle.test", path: "/", httpOnly: true, sameSite: "Lax" },
    ]);
  });

  it("accepts the bare cookie header a harness hands back", () => {
    // schedulerHarness() exposes `ownerSession` as the cookie string, and
    // passing it here used to throw on `.split` of an undefined field.
    expect(browserSessionCookies("session-id=harness; csrftoken=csrf-h").map((c) => c.name)).toEqual([
      "session-id",
      "csrftoken",
    ]);
  });

  it("returns no cookies instead of throwing when the header is missing or blank", () => {
    expect(
      browserSessionCookies({
        email: "a@example.com",
        password: "pw",
        userId: "u",
        cookie: undefined as never,
        csrfToken: "",
        apiBase: "",
      })
    ).toEqual([]);
    expect(browserSessionCookies("   ")).toEqual([]);
  });
});
