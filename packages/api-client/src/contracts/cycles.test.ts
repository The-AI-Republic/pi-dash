// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub proofs for every cycles contract (NEWFRONT-706). One test per
// contract: the URL and method the caller emits, the request body where
// there is one, and that the declared schema accepts the serializer's
// shape and rejects a shape missing its required core. Live suite:
// src/contract/cycles.contract.test.ts.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  addCycleFavorite,
  archiveCycle,
  checkCycleDates,
  createCycle,
  deleteCycle,
  getArchivedCycle,
  getCycle,
  getCycleAnalytics,
  getCycleDetailedProgress,
  getCycleProgress,
  listActiveCycles,
  listArchivedCycles,
  listCycleRows,
  listCycles,
  listWorkspaceCycles,
  removeCycleFavorite,
  restoreCycle,
  transferCycleIssues,
  updateCycle,
} from "./cycles.js";
import { listCycleIssues, listGroupedCycleIssues, listSubGroupedCycleIssues } from "./issues.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "123e4567-e89b-12d3-a456-426614174000";
const TARGET = "323e4567-e89b-12d3-a456-426614174002";

interface Call {
  method?: string;
  url: string;
  json?: unknown;
  query?: unknown;
}

function stubClient(body: unknown): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const record =
    (method: string) =>
    async (url: string, options: { json?: unknown; query?: unknown } = {}) => {
      calls.push({ method, url, json: options.json, query: options.query });
      return method === "GET" ? body : { data: body, status: 200, url };
    };
  // `sendJson` is the client's method name; record it as "JSON" so the
  // assertions read as the verb the caller hands to the client.
  const sendJson = (async (method: string, url: string, options: { json?: unknown; query?: unknown } = {}) => {
    calls.push({ method, url, json: options.json, query: options.query });
    return { data: body, status: 200, url };
  }) as ApiClient["sendJson"];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: record("GET") as ApiClient["getJson"],
    sendJson,
    post: record("POST") as ApiClient["post"],
    delete: record("DELETE") as ApiClient["delete"],
  } as ApiClient;
  return { client, calls };
}

/** A row in the shape the project list serializer actually projects. */
const ROW = {
  id: CYCLE,
  workspace_id: "423e4567-e89b-12d3-a456-426614174003",
  project_id: PROJECT,
  name: "Sprint 1",
  description: "",
  start_date: "2026-10-01T00:00:00+09:00",
  end_date: "2026-10-14T00:00:00+09:00",
  owned_by_id: "523e4567-e89b-12d3-a456-426614174004",
  sort_order: 65535,
  external_source: null,
  external_id: null,
  progress_snapshot: {},
  logo_props: {},
  is_favorite: false,
  total_issues: 4,
  completed_issues: 1,
  cancelled_issues: 0,
  started_issues: 2,
  unstarted_issues: 1,
  backlog_issues: 0,
  assignee_ids: [],
  status: "CURRENT",
  version: 1,
  created_by: "523e4567-e89b-12d3-a456-426614174004",
};

/** The exact key set `GET WS/cycles/` returns: no favorite, status or lead. */
const WORKSPACE_ROW = {
  id: CYCLE,
  workspace_id: ROW.workspace_id,
  project_id: PROJECT,
  name: "Sprint 1",
  description: "",
  start_date: "2026-10-01T00:00:00+09:00",
  end_date: "2026-10-14T00:00:00+09:00",
  owned_by_id: "523e4567-e89b-12d3-a456-426614174004",
  view_props: {},
  sort_order: 65535,
  external_source: null,
  external_id: null,
  progress_snapshot: {},
  logo_props: {},
  total_issues: 4,
  cancelled_issues: 0,
  completed_issues: 1,
  started_issues: 2,
  unstarted_issues: 1,
  backlog_issues: 0,
};

const DISTRIBUTION = {
  assignees: [
    { display_name: "Ada", assignee_id: "523e4567-e89b-12d3-a456-426614174004", avatar_url: null, total_issues: 4 },
  ],
  labels: [{ label_name: "api", label_id: CYCLE, color: "#ff0000", total_issues: 2 }],
  completion_chart: { "2026-10-01": 4, "2026-10-02": 3, "2026-10-03": null },
};

const PROGRESS = {
  total_issues: 4,
  backlog_issues: 0,
  unstarted_issues: 1,
  started_issues: 2,
  completed_issues: 1,
  cancelled_issues: 0,
  backlog_estimate_points: 0,
  unstarted_estimate_points: 1,
  started_estimate_points: 3,
  completed_estimate_points: 2,
  cancelled_estimate_points: 0,
  total_estimate_points: 6,
};

describe("listCycles (thin picker)", () => {
  it("reads the project route without query params by default", async () => {
    const { client, calls } = stubClient([{ id: CYCLE, project_id: PROJECT, name: "Sprint 1", is_favorite: false }]);
    const listed = await listCycles(client, WS, PROJECT);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/`);
    expect(listed[0]).toEqual({ id: CYCLE, project_id: PROJECT, name: "Sprint 1", is_favorite: false });
  });

  it("passes cycle_view through and keeps status optional", async () => {
    const { client, calls } = stubClient([{ ...ROW }]);
    const listed = await listCycles(client, WS, PROJECT, { cycleView: "current", orderBy: "-created_at" });
    expect(calls[0]?.query).toEqual({
      cycle_view: "current",
      order_by: "-created_at",
      per_page: undefined,
      cursor: undefined,
      fields: undefined,
    });
    expect(listed[0]?.status).toBe("CURRENT");
  });
});

describe("listCycleRows", () => {
  it("parses the bare array of full rows", async () => {
    const { client } = stubClient([ROW]);
    const rows = await listCycleRows(client, WS, PROJECT);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.total_issues).toBe(4);
    expect(rows[0]?.assignee_ids).toEqual([]);
  });

  it("accepts a row whose optional projections are absent", async () => {
    const { client } = stubClient([
      {
        id: CYCLE,
        workspace_id: ROW.workspace_id,
        project_id: PROJECT,
        name: "Sprint 1",
        start_date: null,
        end_date: null,
        is_favorite: true,
      },
    ]);
    const rows = await listCycleRows(client, WS, PROJECT);
    expect(rows[0]?.is_favorite).toBe(true);
  });

  it("rejects a row missing its id", async () => {
    const { client } = stubClient([{ ...ROW, id: undefined }]);
    await expect(listCycleRows(client, WS, PROJECT)).rejects.toThrow();
  });
});

describe("createCycle", () => {
  it("posts the create body to the project route", async () => {
    const { client, calls } = stubClient(ROW);
    const created = await createCycle(client, WS, PROJECT, {
      name: "Sprint 1",
      description: "",
      start_date: "2026-10-01",
      end_date: "2026-10-14",
    });
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/`,
      json: { name: "Sprint 1", description: "", start_date: "2026-10-01", end_date: "2026-10-14" },
    });
    expect(created.id).toBe(CYCLE);
  });
});

describe("getCycle", () => {
  it("reads the detail route and parses the row", async () => {
    const { client, calls } = stubClient({ ...ROW, sub_issues: 0 });
    const cycle = await getCycle(client, WS, PROJECT, CYCLE);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/`);
    expect(cycle.sub_issues).toBe(0);
  });
});

describe("updateCycle", () => {
  it("patches the cycle and parses the echo", async () => {
    const { client, calls } = stubClient({
      id: CYCLE,
      project_id: PROJECT,
      name: "Sprint 1 renamed",
      description: "",
      start_date: null,
      end_date: null,
    });
    const updated = await updateCycle(client, WS, PROJECT, CYCLE, { name: "Sprint 1 renamed" });
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/`,
    });
    expect(updated.name).toBe("Sprint 1 renamed");
  });

  it("sends dates to clear them and owner changes", async () => {
    const { client, calls } = stubClient({
      id: CYCLE,
      project_id: PROJECT,
      name: "Sprint 1",
      start_date: null,
      end_date: null,
    });
    await updateCycle(client, WS, PROJECT, CYCLE, { start_date: null, end_date: null, owned_by: TARGET });
    expect(calls[0]?.json).toEqual({ start_date: null, end_date: null, owned_by: TARGET });
  });
});

describe("deleteCycle", () => {
  it("deletes the detail route", async () => {
    const { client, calls } = stubClient(undefined);
    await deleteCycle(client, WS, PROJECT, CYCLE);
    expect(calls[0]).toEqual({
      method: "DELETE",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/`,
      json: undefined,
      query: undefined,
    });
  });
});

describe("listCycleIssues", () => {
  it("reads the cycle-issues route with the issue list query", async () => {
    const page = {
      grouped_by: null,
      sub_grouped_by: null,
      total_count: 0,
      next_cursor: "1000:0:0",
      prev_cursor: "1000:-1:0",
      next_page_results: false,
      prev_page_results: false,
      count: 0,
      total_results: 0,
      results: [],
    };
    const { client, calls } = stubClient(page);
    const listed = await listCycleIssues(client, WS, PROJECT, CYCLE, { perPage: 10, filters: { priority: "high" } });
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/cycle-issues/`);
    expect(calls[0]?.query).toMatchObject({ per_page: 10, priority: "high" });
    expect(listed.results).toEqual([]);
  });
});

describe("checkCycleDates", () => {
  it("posts date-only values and parses the free-range answer", async () => {
    const { client, calls } = stubClient({ status: true });
    const result = await checkCycleDates(client, WS, PROJECT, {
      start_date: "2026-10-01",
      end_date: "2026-10-14",
    });
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/date-check/`,
      json: { start_date: "2026-10-01", end_date: "2026-10-14" },
    });
    expect(result.status).toBe(true);
  });

  it("keeps the guidance message on a collision", async () => {
    const message = "You have a cycle already on the given dates";
    const { client } = stubClient({ status: false, error: message });
    const result = await checkCycleDates(client, WS, PROJECT, {
      start_date: "2026-10-01",
      end_date: "2026-10-14",
      cycle_id: CYCLE,
    });
    expect(result).toEqual({ status: false, error: message });
  });
});

describe("transferCycleIssues", () => {
  it("posts the target cycle to the transfer route and parses the message", async () => {
    const { client, calls } = stubClient({ message: "Work items have been transferred successfully" });
    const result = await transferCycleIssues(client, WS, PROJECT, CYCLE, TARGET);
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/transfer-issues/`,
      json: { new_cycle_id: TARGET },
      query: undefined,
    });
    expect(result.message).toBe("Work items have been transferred successfully");
  });

  it("rejects a result without a message", async () => {
    const { client } = stubClient({ success: true });
    await expect(transferCycleIssues(client, WS, PROJECT, CYCLE, TARGET)).rejects.toThrow();
  });
});

describe("project cycle favourites", () => {
  it("adds with {cycle} and removes by cycle id", async () => {
    const { client, calls } = stubClient(undefined);
    await addCycleFavorite(client, WS, PROJECT, CYCLE);
    await removeCycleFavorite(client, WS, PROJECT, CYCLE);
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/user-favorite-cycles/`,
      json: { cycle: CYCLE },
    });
    expect(calls[1]).toEqual({
      method: "DELETE",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/user-favorite-cycles/${CYCLE}/`,
      json: undefined,
      query: undefined,
    });
  });
});

describe("archive and restore", () => {
  it("archives with an empty POST and parses the stamp", async () => {
    const { client, calls } = stubClient({ archived_at: "2026-10-15 00:00:00+00:00" });
    const result = await archiveCycle(client, WS, PROJECT, CYCLE);
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/archive/`,
      json: undefined,
      query: undefined,
    });
    expect(result.archived_at).toContain("2026-10-15");
  });

  it("restores with DELETE on the same route", async () => {
    const { client, calls } = stubClient(null);
    const status = await restoreCycle(client, WS, PROJECT, CYCLE);
    expect(calls[0]).toEqual({
      method: "DELETE",
      url: `/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/archive/`,
      json: undefined,
      query: undefined,
    });
    expect(status).toBe(200);
  });
});

describe("archived reads", () => {
  it("lists archived cycles from the bare array", async () => {
    const { client, calls } = stubClient([
      {
        id: CYCLE,
        workspace_id: ROW.workspace_id,
        project_id: PROJECT,
        name: "Archived cycle",
        description: "",
        start_date: "2026-09-01T00:00:00Z",
        end_date: "2026-09-30T00:00:00Z",
        owned_by_id: ROW.owned_by_id,
        status: "COMPLETED",
        archived_at: "2026-10-15T00:00:00Z",
        is_favorite: false,
        total_issues: 4,
        completed_issues: 1,
        started_issues: 2,
        unstarted_issues: 1,
        cancelled_issues: 0,
        backlog_issues: 0,
        assignee_ids: [],
      },
    ]);
    const rows = await listArchivedCycles(client, WS, PROJECT);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/archived-cycles/`);
    expect(rows[0]?.archived_at).toBe("2026-10-15T00:00:00Z");
  });

  it("parses the archived detail with its nested distribution", async () => {
    const { client, calls } = stubClient({
      id: CYCLE,
      workspace_id: ROW.workspace_id,
      project_id: PROJECT,
      name: "Archived cycle",
      description: "",
      start_date: "2026-09-01T00:00:00Z",
      end_date: "2026-09-30T00:00:00Z",
      owned_by_id: ROW.owned_by_id,
      status: "COMPLETED",
      archived_at: "2026-10-15T00:00:00Z",
      is_favorite: false,
      total_issues: 4,
      completed_issues: 2,
      started_issues: 1,
      unstarted_issues: 1,
      cancelled_issues: 0,
      backlog_issues: 0,
      assignee_ids: [],
      sub_issues: 1,
      completed_estimate_points: 2,
      total_estimate_points: 6,
      estimate_distribution: {},
      distribution: DISTRIBUTION,
    });
    const detail = await getArchivedCycle(client, WS, PROJECT, CYCLE);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/archived-cycles/${CYCLE}/`);
    expect(detail.distribution.assignees).toEqual(DISTRIBUTION.assignees);
  });
  it("accepts the point-estimate distribution on archived detail", async () => {
    const { client } = stubClient({
      id: CYCLE,
      workspace_id: ROW.workspace_id,
      project_id: PROJECT,
      name: "Archived cycle",
      description: "",
      start_date: "2026-09-01T00:00:00Z",
      end_date: "2026-09-30T00:00:00Z",
      owned_by_id: ROW.owned_by_id,
      status: "COMPLETED",
      archived_at: "2026-10-15T00:00:00Z",
      is_favorite: false,
      total_issues: 4,
      completed_issues: 2,
      started_issues: 1,
      unstarted_issues: 1,
      cancelled_issues: 0,
      backlog_issues: 0,
      assignee_ids: [],
      sub_issues: 1,
      completed_estimate_points: 2,
      total_estimate_points: 6,
      estimate_distribution: {
        assignees: [{ display_name: "Ada", assignee_id: null, total_estimates: 6, completed_estimates: 2 }],
        labels: [{ label_name: "api", label_id: null, total_estimates: 6, pending_estimates: 4 }],
        completion_chart: { "2026-10-01": 6, "2026-10-02": null },
      },
      distribution: DISTRIBUTION,
    });
    const detail = await getArchivedCycle(client, WS, PROJECT, CYCLE);
    expect(detail.estimate_distribution).toMatchObject({ completion_chart: { "2026-10-01": 6 } });
  });

  it("rejects a malformed estimate assignee row", async () => {
    const { client } = stubClient({
      id: CYCLE,
      workspace_id: ROW.workspace_id,
      project_id: PROJECT,
      name: "Archived cycle",
      description: "",
      start_date: "2026-09-01T00:00:00Z",
      end_date: "2026-09-30T00:00:00Z",
      owned_by_id: ROW.owned_by_id,
      status: "COMPLETED",
      archived_at: "2026-10-15T00:00:00Z",
      is_favorite: false,
      total_issues: 4,
      completed_issues: 2,
      started_issues: 1,
      unstarted_issues: 1,
      cancelled_issues: 0,
      backlog_issues: 0,
      assignee_ids: [],
      sub_issues: 1,
      completed_estimate_points: 2,
      total_estimate_points: 6,
      estimate_distribution: { assignees: "Ada", labels: [], completion_chart: {} },
      distribution: DISTRIBUTION,
    });
    await expect(getArchivedCycle(client, WS, PROJECT, CYCLE)).rejects.toThrow();
  });

  it("rejects an estimate distribution with arbitrary keys", async () => {
    const { client } = stubClient({
      id: CYCLE,
      workspace_id: ROW.workspace_id,
      project_id: PROJECT,
      name: "Archived cycle",
      description: "",
      start_date: "2026-09-01T00:00:00Z",
      end_date: "2026-09-30T00:00:00Z",
      owned_by_id: ROW.owned_by_id,
      status: "COMPLETED",
      archived_at: "2026-10-15T00:00:00Z",
      is_favorite: false,
      total_issues: 4,
      completed_issues: 2,
      started_issues: 1,
      unstarted_issues: 1,
      cancelled_issues: 0,
      backlog_issues: 0,
      assignee_ids: [],
      sub_issues: 1,
      completed_estimate_points: 2,
      total_estimate_points: 6,
      estimate_distribution: { surprise: "not part of the serializer" },
      distribution: DISTRIBUTION,
    });
    await expect(getArchivedCycle(client, WS, PROJECT, CYCLE)).rejects.toThrow();
  });
});

describe("workspace cycle reads", () => {
  it("lists the narrower workspace projection", async () => {
    const { client, calls } = stubClient([WORKSPACE_ROW]);
    const rows = await listWorkspaceCycles(client, WS);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/cycles/`);
    expect(rows[0]?.project_id).toBe(PROJECT);
    expect(rows[0]?.total_issues).toBe(4);
    expect(Object.keys(rows[0] ?? {})).toEqual(Object.keys(WORKSPACE_ROW));
  });

  it("rejects a workspace row with the project-list-only fields", async () => {
    const { client } = stubClient([{ ...WORKSPACE_ROW, is_favorite: false }]);
    await expect(listWorkspaceCycles(client, WS)).rejects.toThrow();
  });

  it("rejects a workspace row missing its counters", async () => {
    const { client } = stubClient([{ ...WORKSPACE_ROW, total_issues: undefined }]);
    await expect(listWorkspaceCycles(client, WS)).rejects.toThrow();
  });

  it("pages workspace active cycles", async () => {
    const page = {
      count: 1,
      next_cursor: null,
      prev_cursor: null,
      results: [ROW],
      next_page_results: false,
      prev_page_results: false,
      total_pages: 1,
    };
    const { client, calls } = stubClient(page);
    const listed = await listActiveCycles(client, WS, { perPage: 20, cursor: "abc" });
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/active-cycles/`);
    expect(calls[0]?.query).toEqual({ per_page: 20, cursor: "abc" });
    expect(listed.results).toHaveLength(1);
  });

  it("rejects an active-cycle page without its results array", async () => {
    const { client } = stubClient({ count: 0, next_cursor: null, prev_cursor: null });
    await expect(listActiveCycles(client, WS)).rejects.toThrow();
  });
});

describe("progress reads", () => {
  it("reads the progress route and parses counts plus estimate totals", async () => {
    const { client, calls } = stubClient(PROGRESS);
    const progress = await getCycleProgress(client, WS, PROJECT, CYCLE);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/progress/`);
    expect(progress.total_estimate_points).toBe(6);
  });

  it("rejects a payload without the transferable counts", async () => {
    const { client } = stubClient({ total_issues: 3 });
    await expect(getCycleProgress(client, WS, PROJECT, CYCLE)).rejects.toThrow();
  });

  it("reads the cloud cycle-progress route", async () => {
    const { client, calls } = stubClient({
      ...PROGRESS,
      distribution: DISTRIBUTION,
      progress: [
        { date: "2026-10-01", value: 4 },
        { date: "2026-10-02", value: null },
      ],
    });
    const detail = await getCycleDetailedProgress(client, WS, PROJECT, CYCLE);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/cycle-progress/`);
    expect(detail.progress?.[1]?.value).toBeNull();
  });
});

describe("getCycleAnalytics", () => {
  it("requests the issues measure by default and parses the distributions", async () => {
    const { client, calls } = stubClient(DISTRIBUTION);
    const analytics = await getCycleAnalytics(client, WS, PROJECT, CYCLE);
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/analytics/`);
    expect(calls[0]?.query).toEqual({ type: "issues" });
    expect(analytics.assignees[0]?.display_name).toBe("Ada");
    expect(analytics.completion_chart["2026-10-02"]).toBe(3);
  });

  it("requests the points measure when asked", async () => {
    const { client, calls } = stubClient(DISTRIBUTION);
    await getCycleAnalytics(client, WS, PROJECT, CYCLE, "points");
    expect(calls[0]?.query).toEqual({ type: "points" });
  });

  it("rejects a payload without the three distribution keys", async () => {
    const { client } = stubClient({ assignees: [] });
    await expect(getCycleAnalytics(client, WS, PROJECT, CYCLE)).rejects.toThrow();
  });
});

describe("cycle issue list variants (shared issues contracts)", () => {
  const emptyPage = {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "1000:0:0",
    prev_cursor: "1000:-1:0",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: [],
  };

  it("reads the grouped cycle-issues bucket map", async () => {
    const { client, calls } = stubClient({ ...emptyPage, grouped_by: "priority", results: {} });
    const page = await listGroupedCycleIssues(client, WS, PROJECT, CYCLE, { groupBy: "priority" });
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/cycle-issues/`);
    expect(calls[0]?.query).toMatchObject({ group_by: "priority" });
    expect(page.grouped_by).toBe("priority");
  });

  it("reads the sub-grouped cycle-issues bucket map", async () => {
    const { client, calls } = stubClient({
      ...emptyPage,
      grouped_by: "priority",
      sub_grouped_by: "state_id",
      results: {},
    });
    const page = await listSubGroupedCycleIssues(client, WS, PROJECT, CYCLE, {
      groupBy: "priority",
      subGroupBy: "state_id",
    });
    expect(calls[0]?.url).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/cycles/${CYCLE}/cycle-issues/`);
    expect(calls[0]?.query).toMatchObject({ group_by: "priority", sub_group_by: "state_id" });
    expect(page.sub_grouped_by).toBe("state_id");
  });
});
