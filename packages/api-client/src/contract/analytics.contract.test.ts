// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the analytics reads against live Django. The seed plants a
// dated and an undated cycle/module pair plus a January probe issue, so the
// suite can prove the workspace monthly series, the scoped daily series and
// the empty-series answer for an undated scope. Rows are resolved by name,
// never by position.
import { describe, expect, it } from "vitest";
import { isApiError } from "../errors.js";
import {
  ANALYTICS_TABS,
  getAnalyticsChart,
  getAnalyticsStats,
  getAnalyticsTotals,
  getProjectAnalyticsChart,
  getProjectAnalyticsStats,
  getProjectAnalyticsTotals,
  getProjectStats,
} from "../contracts/analytics.js";
import { listCycles } from "../contracts/cycles.js";
import { listModules } from "../contracts/modules.js";
import { getProject, listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function scopeId(client: ApiClient, projectId: string, kind: "cycle" | "module", name: string): Promise<string> {
  const rows =
    kind === "cycle"
      ? await listCycles(client, CONTRACT_WORKSPACE, projectId)
      : await listModules(client, CONTRACT_WORKSPACE, projectId);
  const found = rows.find((row) => row.name === name);
  expect(found).toBeDefined();
  return found?.id ?? "";
}

function metric(value: { count?: number | null | undefined } | undefined): unknown {
  return value?.count;
}

describe.skipIf(!contractEnabled)("analytics read contract", () => {
  it("answers the overview totals with every headline metric", async () => {
    const client = await sharedSignedIn();
    const totals = await getAnalyticsTotals(client, CONTRACT_WORKSPACE, { tab: "overview" });
    for (const key of [
      "total_users",
      "total_admins",
      "total_members",
      "total_guests",
      "total_projects",
      "total_work_items",
      "total_cycles",
      "total_intake",
      "agent_run_input_tokens",
      "agent_run_output_tokens",
      "agent_run_total_tokens",
    ] as const) {
      expect(typeof totals[key]?.count).toBe("number");
    }
    expect(metric(totals.total_projects)).toBeGreaterThanOrEqual(2);
    expect(metric(totals.total_work_items)).toBeGreaterThanOrEqual(2);
  });

  it("answers the work-items totals and defaults the tab to overview", async () => {
    const client = await sharedSignedIn();
    const items = await getAnalyticsTotals(client, CONTRACT_WORKSPACE, { tab: "work-items" });
    expect(typeof metric(items.total_work_items)).toBe("number");
    expect(typeof metric(items.started_work_items)).toBe("number");
    expect(typeof metric(items.backlog_work_items)).toBe("number");
    expect(typeof metric(items.un_started_work_items)).toBe("number");
    expect(typeof metric(items.completed_work_items)).toBe("number");
    expect(metric(items.total_work_items)).toBeGreaterThanOrEqual(2);
    const withExtras =
      (items.started_work_items?.count ?? 0) +
      (items.backlog_work_items?.count ?? 0) +
      (items.un_started_work_items?.count ?? 0) +
      (items.completed_work_items?.count ?? 0);
    expect(withExtras).toBeLessThanOrEqual(metric(items.total_work_items) as number);

    // The tab defaults to overview: the same eleven keys as the tab above.
    const defaulted = await getAnalyticsTotals(client, CONTRACT_WORKSPACE);
    expect(metric(defaulted.total_projects)).toBe(6 - 4);
  });

  it("scopes the totals to the picker's project ids", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const all = await getAnalyticsTotals(client, CONTRACT_WORKSPACE, { tab: "overview" });
    const scoped = await getAnalyticsTotals(client, CONTRACT_WORKSPACE, {
      tab: "overview",
      projectIds: [projectId],
    });
    expect(metric(scoped.total_projects)).toBe(1);
    expect(metric(scoped.total_work_items)).toBeLessThanOrEqual(metric(all.total_work_items) as number);
    expect(Object.keys(scoped)).toEqual(Object.keys(all));
  });

  it("refuses an unknown tab with 400", async () => {
    const client = await sharedSignedIn();
    const refusal = await getAnalyticsTotals(client, CONTRACT_WORKSPACE, {
      tab: "bogus" as unknown as (typeof ANALYTICS_TABS)[number],
    }).catch((error: unknown) => error);
    expect(isApiError(refusal) && refusal.status).toBe(400);
  });

  it("answers the seven project radar rows in server order", async () => {
    const client = await sharedSignedIn();
    const chart = await getAnalyticsChart(client, CONTRACT_WORKSPACE, { type: "projects" });
    expect(chart.map((row) => row.key)).toEqual([
      "work_items",
      "cycles",
      "modules",
      "intake",
      "members",
      "pages",
      "views",
    ]);
    expect(chart.map((row) => row.name)).toEqual([
      "Work Items",
      "Cycles",
      "Modules",
      "Intake",
      "Members",
      "Pages",
      "Views",
    ]);
    for (const row of chart) expect(typeof row.count).toBe("number");
    const scopedRow = chart.find((row) => row.key === "work_items");
    expect(scopedRow?.count).toBeGreaterThanOrEqual(2);
  });

  it("answers a monthly created-vs-resolved series from workspace creation", async () => {
    const client = await sharedSignedIn();
    const chart = await getAnalyticsChart(client, CONTRACT_WORKSPACE, { type: "work-items" });
    // One bucket per month from the workspace's creation month through the
    // current month, including zero-count months; a member read always has
    // at least the current month.
    const now = new Date();
    const currentMonth = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}-01`;
    expect(chart.data.length).toBeGreaterThanOrEqual(1);
    expect(chart.data.at(-1)?.key).toBe(currentMonth);
    expect(chart.data.at(-1)?.name).toBe(currentMonth);
    for (const point of chart.data) {
      expect(point.key).toMatch(/^\d{4}-\d{2}-01$/);
      expect(point.count).toBe(point.created_issues);
      expect(typeof point.completed_issues).toBe("number");
    }
    // The workspace series sums a month's creations; the seeded project
    // holds at least the two contract issues.
    const created = chart.data.reduce((sum, point) => sum + point.created_issues, 0);
    expect(created).toBeGreaterThanOrEqual(2);
    expect(chart.schema).toEqual({
      completed_issues: "completed_issues",
      created_issues: "created_issues",
    });
  });

  it("answers custom bars without a split and with a stacked split", async () => {
    const client = await sharedSignedIn();
    const plain = await getAnalyticsChart(client, CONTRACT_WORKSPACE, {
      type: "custom-work-items",
      xAxis: "STATE_GROUPS",
    });
    expect(plain.schema).toEqual({});
    expect(plain.data.length).toBeGreaterThan(0);
    const backlog = plain.data.find((row) => row.key === "backlog");
    expect(backlog).toBeDefined();
    expect(backlog?.name).toBe("backlog");
    expect(typeof backlog?.count).toBe("number");
    expect(Object.keys(backlog ?? {}).sort()).toEqual(["count", "key", "name"]);

    const split = await getAnalyticsChart(client, CONTRACT_WORKSPACE, {
      type: "custom-work-items",
      xAxis: "STATE_GROUPS",
      groupBy: "PRIORITY",
    });
    expect(Object.keys(split.schema).length).toBeGreaterThan(0);
    for (const row of split.data) {
      const segments = Object.keys(split.schema).reduce((sum, key) => sum + Number(row[key] ?? 0), 0);
      expect(segments).toBe(row.count);
    }
  });

  it("refuses an unknown chart axis or type with 400", async () => {
    const client = await sharedSignedIn();
    const axis = await getAnalyticsChart(client, CONTRACT_WORKSPACE, {
      type: "custom-work-items",
      xAxis: "NOPE" as unknown as "STATES",
    }).catch((error: unknown) => error);
    expect(isApiError(axis) && axis.status).toBe(400);
    const type = await getAnalyticsChart(client, CONTRACT_WORKSPACE, {
      type: "bogus" as unknown as "projects",
    }).catch((error: unknown) => error);
    expect(isApiError(type) && type.status).toBe(400);
  });

  it("answers workspace state-breakdown rows per project", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const rows = await getAnalyticsStats(client, CONTRACT_WORKSPACE, { type: "work-items" });
    const seeded = rows.find((row) => row.project_id === projectId);
    expect(seeded).toBeDefined();
    expect(seeded?.project__name).toBe("Contract");
    const groups =
      (seeded?.backlog_work_items ?? 0) +
      (seeded?.un_started_work_items ?? 0) +
      (seeded?.started_work_items ?? 0) +
      (seeded?.completed_work_items ?? 0) +
      (seeded?.cancelled_work_items ?? 0);
    expect(groups).toBeGreaterThanOrEqual(2);
    const ids = rows.map((row) => row.project_id);
    expect([...ids].sort()).toEqual(ids);
  });

  it("answers per-project totals for the active-projects list", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const rows = await getProjectStats(client, CONTRACT_WORKSPACE, {
      fields: ["total_issues", "completed_issues"],
    });
    const seeded = rows.find((row) => row.id === projectId);
    expect(seeded).toBeDefined();
    expect(typeof seeded?.total_issues).toBe("number");
    expect(typeof seeded?.completed_issues).toBe("number");
    expect(seeded?.total_issues).toBeGreaterThanOrEqual(2);
    expect(seeded).not.toHaveProperty("total_cycles");

    const scoped = await getProjectStats(client, CONTRACT_WORKSPACE, {
      fields: ["total_issues"],
      projectIds: [projectId],
    });
    expect(scoped).toHaveLength(1);
    expect(scoped[0]?.id).toBe(projectId);
  });

  it("falls back to every field when none of the requested names is valid", async () => {
    const client = await sharedSignedIn();
    const rows = await getProjectStats(client, CONTRACT_WORKSPACE, {
      fields: ["bogus" as unknown as "total_issues"],
    });
    expect(rows.length).toBeGreaterThan(0);
    for (const key of ["total_issues", "completed_issues", "total_members", "total_cycles", "total_modules"]) {
      expect(typeof rows[0]?.[key]).toBe("number");
    }
  });

  it("answers the project-scoped totals, with and without cycle/module scope", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const whole = await getProjectAnalyticsTotals(client, CONTRACT_WORKSPACE, projectId, {
      projectIds: [projectId],
    });
    expect(whole.total_work_items?.count).toBeGreaterThanOrEqual(2);

    const otherProject = (await listProjects(client, CONTRACT_WORKSPACE)).find((project) => project.id !== projectId);
    expect(otherProject).toBeDefined();
    const empty = await getProjectAnalyticsTotals(client, CONTRACT_WORKSPACE, otherProject?.id ?? "");
    expect(empty.total_work_items?.count).toBe(0);
    expect(empty.started_work_items?.count).toBe(0);

    const cycleId = await scopeId(client, projectId, "cycle", "Contract analytics cycle");
    const byCycle = await getProjectAnalyticsTotals(client, CONTRACT_WORKSPACE, projectId, { cycleId });
    expect(byCycle.total_work_items?.count).toBe(1);
    expect(byCycle.backlog_work_items?.count).toBe(1);

    const moduleId = await scopeId(client, projectId, "module", "Contract analytics module");
    const byModule = await getProjectAnalyticsTotals(client, CONTRACT_WORKSPACE, projectId, { moduleId });
    expect(byModule.total_work_items?.count).toBe(1);
  });

  it("answers the project-scoped daily series and an empty series for an undated scope", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const whole = await getProjectAnalyticsChart(client, CONTRACT_WORKSPACE, projectId, {
      type: "work-items",
    });
    expect(whole.data.length).toBeGreaterThanOrEqual(1);
    expect(whole.schema).toHaveProperty("created_issues");

    const cycleId = await scopeId(client, projectId, "cycle", "Contract analytics cycle");
    const byCycle = await getProjectAnalyticsChart(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      { type: "work-items" },
      { cycleId }
    );
    expect(byCycle.data).toHaveLength(31);
    expect(byCycle.data[0]?.name).toBe("2026-01-01");
    expect(byCycle.data.at(-1)?.name).toBe("2026-01-31");
    for (const point of byCycle.data) expect(typeof point.completed_issues).toBe("number");

    const moduleId = await scopeId(client, projectId, "module", "Contract analytics module");
    const byModule = await getProjectAnalyticsChart(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      { type: "work-items" },
      { moduleId }
    );
    expect(byModule.data).toHaveLength(31);

    const undatedCycle = await scopeId(client, projectId, "cycle", "Contract cycle");
    const empty = await getProjectAnalyticsChart(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      { type: "work-items" },
      { cycleId: undatedCycle }
    );
    expect(empty).toEqual({ data: [], schema: {} });
  });

  it("answers the project-scoped custom chart", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const chart = await getProjectAnalyticsChart(client, CONTRACT_WORKSPACE, projectId, {
      type: "custom-work-items",
      xAxis: "PRIORITY",
      projectIds: [projectId],
    });
    expect(chart.data.length).toBeGreaterThan(0);
    expect(chart.schema).toEqual({});
    expect(chart.data.every((row) => typeof row.count === "number")).toBe(true);
  });

  it("answers the project-scoped assignee breakdown with its unassigned group", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const rows = await getProjectAnalyticsStats(client, CONTRACT_WORKSPACE, projectId, {
      type: "work-items",
      projectIds: [projectId],
    });
    expect(rows.length).toBeGreaterThan(0);
    const unassigned = rows.find((row) => row.assignee_id === null);
    expect(unassigned).toBeDefined();
    expect(unassigned?.display_name).toBeNull();
    expect(unassigned?.avatar_url).toBeNull();
    const assigned = rows.find((row) => row.assignee_id !== null);
    expect(assigned?.display_name).toBeTypeOf("string");
  });

  it("reads the seeded project through the project detail contract", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const project = await getProject(client, CONTRACT_WORKSPACE, projectId);
    expect(project.id).toBe(projectId);
  });
});
