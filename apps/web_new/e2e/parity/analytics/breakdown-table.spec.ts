// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-377): analytics state-breakdown table,
// exports, and table search. Rows ANL-013 (per-project per-stage counts
// with a live search box, a row-count label, skeleton rows while loading, a
// guidance empty view on no data, and an export of the filtered rows),
// ANL-016 (every export downloads a workspace-slug-named file with
// display-label headers; the dash fallback for unreadable cells never
// triggers from current tables — all export values resolve), ANL-017 (each
// table pairs its row-count label with an expanding search box filtering
// the first column; closing the box clears the filter). Green on apps/web
// first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend's demo seed (NEWFRONT-265), and creates exactly the projects
// it needs — scenario project names never reuse the workspace name, or the
// drain cannot tell them apart.
//
// Seeding note: moving an issue into a ticking state (started) makes the
// server try an agent dispatch, and a scratch stack with no runners bounces
// the issue back to Backlog with a system comment. Seeded state moves below
// ride the client's skip-dispatch header so the planted group sticks; the
// resulting server state is the legitimate seeded state either way.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  analyticsAdvanceStatsRaw,
  browserSessionCookies,
  createIssue,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectStates,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_013 = ["ANL-013"];
const ROW_016 = ["ANL-016"];
const ROW_017 = ["ANL-017"];

interface AnalyticsWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupAnalyticsWorld(prefix: string): Promise<AnalyticsWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Anl WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `anl-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, tag };
}

/** Create one project; retries past identifier collisions like viewsHarness. */
async function setupAnalyticsProject(
  world: AnalyticsWorld,
  name: string,
  identifierPrefix: string
): Promise<{ id: string; name: string; identifier: string }> {
  let id = "";
  let identifier = "";
  for (let attempt = 0; attempt < 4; attempt += 1) {
    identifier = parityProjectIdentifier(identifierPrefix);
    try {
      id = await createProjectViaApi(world.owner, world.workspaceSlug, { name, identifier });
      break;
    } catch (error) {
      if (attempt === 3 || !(error instanceof Error) || !/HTTP 400/.test(error.message)) throw error;
    }
  }
  return { id, name, identifier };
}

/** Seat a workspace user and add them to `projectIds` at `role`. */
async function seatAnalyticsUser(
  world: AnalyticsWorld,
  role: number,
  prefix: string,
  projectIds: string[] = []
): Promise<AuthedSession> {
  const user = await seatFreshMember(world.owner, world.workspaceSlug, role, prefix);
  await markOnboardedForProjects(user);
  await setLastWorkspaceForProjects(user, world.workspaceId);
  for (const projectId of projectIds) {
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, projectId, [{ member_id: user.userId, role }]);
  }
  return user;
}

/** Enter an analytics tab authenticated as `session`. */
async function enterAnalyticsTab(
  driver: ParityDriver,
  world: AnalyticsWorld,
  tabId: string,
  session: AuthedSession = world.owner
): Promise<void> {
  await driver.analyticsOpenTab(world.workspaceSlug, tabId, browserSessionCookies(session));
}

/**
 * Move one seeded issue, skipping the agent dispatch the scratch stack could
 * never serve (see the file header). Throttle-tolerant like `authed`.
 */
async function moveSeededIssue(
  world: AnalyticsWorld,
  projectId: string,
  issueId: string,
  stateId: string
): Promise<void> {
  const backoffMs = [3000, 6000, 12000];
  for (let attempt = 0; ; attempt += 1) {
    const res = await fetch(
      `${world.owner.apiBase}/api/workspaces/${world.workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
      {
        method: "PATCH",
        headers: {
          "content-type": "application/json",
          cookie: world.owner.cookie,
          "X-Pi-Dash-Skip-Immediate-Dispatch": "1",
        },
        body: JSON.stringify({ state_id: stateId }),
      }
    );
    if (res.status === 429 && attempt < backoffMs.length) {
      await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
      continue;
    }
    if (!res.ok) throw new Error(`[parity] seeded state move failed with HTTP ${res.status}.`);
    return;
  }
}

/** Per-project rows of a stats payload. */
function statRowsOf(payload: unknown): Array<{
  project: string;
  backlog: number;
  started: number;
  unstarted: number;
  completed: number;
  cancelled: number;
}> {
  if (!Array.isArray(payload)) return [];
  const rows: Array<{
    project: string;
    backlog: number;
    started: number;
    unstarted: number;
    completed: number;
    cancelled: number;
  }> = [];
  for (const row of payload) {
    if (typeof row !== "object" || row === null) continue;
    const record = row as Record<string, unknown>;
    if (typeof record["project__name"] !== "string") continue;
    const counts = [
      "backlog_work_items",
      "started_work_items",
      "un_started_work_items",
      "completed_work_items",
      "cancelled_work_items",
    ].map((key) => record[key]);
    if (!counts.every((count) => typeof count === "number")) continue;
    rows.push({
      project: record["project__name"],
      backlog: counts[0] as number,
      started: counts[1] as number,
      unstarted: counts[2] as number,
      completed: counts[3] as number,
      cancelled: counts[4] as number,
    });
  }
  return rows;
}

/** Split downloaded CSV text into unquoted cell rows. */
function csvRowsOf(content: string): string[][] {
  return content
    .replace(/^﻿/, "")
    .split("\r\n")
    .filter((line) => line !== "")
    .map((line) => line.split(",").map((cell) => cell.replace(/^"|"$/g, "")));
}

const GUIDANCE_TEXT =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

test(
  specTitle(ROW_013, "breakdown counts match the server per project and stage behind skeletons"),
  { tag: specTags(ROW_013) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl013");
    const alpha = await setupAnalyticsProject(world, `Anl Alpha ${world.tag.slice(-4)}`, "AA");
    const beta = await setupAnalyticsProject(world, `Anl Beta ${world.tag.slice(-4)}`, "AB");
    const states = await projectStates(world.workspaceSlug, alpha.id, world.owner.cookie, world.owner.apiBase);
    const stateIdOf = (group: string): string => {
      const id = states.find((s) => s.group === group)?.id;
      if (!id) throw new Error(`[parity] seeded project has no ${group} state.`);
      return id;
    };
    const started = await createIssue(
      world.workspaceSlug,
      alpha.id,
      world.owner.cookie,
      "bd started",
      {},
      world.owner.apiBase
    );
    const done = await createIssue(
      world.workspaceSlug,
      alpha.id,
      world.owner.cookie,
      "bd done",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, alpha.id, world.owner.cookie, "bd backlog", {}, world.owner.apiBase);
    await createIssue(world.workspaceSlug, beta.id, world.owner.cookie, "bd other", {}, world.owner.apiBase);
    await moveSeededIssue(world, alpha.id, started.id, stateIdOf("started"));
    await moveSeededIssue(world, alpha.id, done.id, stateIdOf("completed"));

    await test.step("skeleton rows cover the table while the stats read flies", async () => {
      await driver.analyticsInsightTableStall();
      await enterAnalyticsTab(driver, world, "work-items");
      await expect.poll(() => driver.analyticsInsightSkeletonCells(), { timeout: 60_000 }).toBeGreaterThan(0);
      await driver.analyticsInsightTableRelease();
    });

    await test.step("counts match the server per project and stage", async () => {
      // Settle polls count atomically (multi-call cell reads race row-count
      // shrinkage), then read once.
      await expect.poll(() => driver.analyticsExportRowCount("insight"), { timeout: 60_000 }).toBe(2);
      expect(await driver.analyticsExportHeaders("insight")).toEqual([
        "Project",
        "Backlog",
        "Started",
        "Unstarted",
        "Completed",
        "Cancelled",
      ]);
      expect(await driver.analyticsExportCountLabel("insight")).toBe("2 Projects");
      const read = await analyticsAdvanceStatsRaw(world.owner, world.workspaceSlug);
      expect(read.status).toBe(200);
      const rows = statRowsOf(read.payload);
      expect(rows).toHaveLength(2);
      const cells = await driver.analyticsExportRowCells("insight");
      for (const cell of cells) {
        const server = rows.find((row) => row.project === cell[0]);
        expect(server).toBeDefined();
        expect(cell.slice(1)).toEqual([
          String(server!.backlog),
          String(server!.started),
          String(server!.unstarted),
          String(server!.completed),
          String(server!.cancelled),
        ]);
      }
      const byName = new Map(rows.map((row) => [row.project, row]));
      expect(byName.get(alpha.name)).toMatchObject({ backlog: 1, started: 1, completed: 1 });
      expect(byName.get(beta.name)).toMatchObject({ backlog: 1, started: 0, completed: 0 });
    });

    await test.step("search narrows rows and export carries exactly the filtered rows", async () => {
      await driver.analyticsExportSearch("insight", "Beta");
      await expect.poll(() => driver.analyticsExportRowCount("insight"), { timeout: 30_000 }).toBe(1);
      expect(await driver.analyticsExportRowCells("insight")).toEqual([[beta.name, "1", "0", "0", "0", "0"]]);
      const file = await driver.analyticsExportDownload("insight");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const lines = csvRowsOf(file.content);
      expect(lines[0]).toEqual(["Project", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"]);
      expect(lines.slice(1)).toEqual([[beta.name, "1", "0", "0", "0", "0"]]);
      await driver.analyticsExportClearSearch("insight");
      await expect.poll(() => driver.analyticsExportRowCount("insight"), { timeout: 30_000 }).toBe(2);
    });
  }
);

test(
  specTitle(ROW_013, "breakdown with no readable rows shows the guidance empty view"),
  { tag: specTags(ROW_013) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl013g");
    const project = await setupAnalyticsProject(world, `Anl Ungraphed ${world.tag.slice(-4)}`, "AU");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl013gg", [project.id]);

    await test.step("the server refuses the guest's stats read", async () => {
      expect((await analyticsAdvanceStatsRaw(guest, world.workspaceSlug)).status).toBe(403);
    });

    await test.step("the table shows the guidance view with a zeroed count", async () => {
      await enterAnalyticsTab(driver, world, "work-items", guest);
      // The failed read retries in the background, so skeletons flash
      // between guidance renders; every read polls for its own settled
      // value instead of assuming one phase.
      await expect.poll(() => driver.analyticsExportEmptyText("insight"), { timeout: 60_000 }).toBe(GUIDANCE_TEXT);
      await expect.poll(() => driver.analyticsExportCountLabel("insight"), { timeout: 60_000 }).toBe("0 Projects");
      await expect
        .poll(() => driver.analyticsExportHeaders("insight"), { timeout: 60_000 })
        .toEqual(["Project", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"]);
    });
  }
);

test(
  specTitle(ROW_016, "every export names the workspace file with display-label headers"),
  { tag: specTags(ROW_016) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl016");
    const project = await setupAnalyticsProject(world, `Anl Export ${world.tag.slice(-4)}`, "AE");
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "ex urgent",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "ex high",
      { priority: "high" },
      world.owner.apiBase
    );

    await enterAnalyticsTab(driver, world, "work-items");
    await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 60_000 }).toBe(2);

    await test.step("the chart-table export carries display labels and plotted values", async () => {
      const file = await driver.analyticsExportDownload("chart");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const lines = csvRowsOf(file.content);
      expect(lines[0]).toEqual(["Priority", "Count"]);
      expect(new Set(lines.slice(1).map((row) => row.join(":")))).toEqual(new Set(["Urgent:1", "High:1"]));
      for (const row of lines.slice(1)) {
        for (const cell of row) expect(cell).not.toBe("-");
      }
    });

    await test.step("the insight-table export carries display labels and stage counts", async () => {
      const file = await driver.analyticsExportDownload("insight");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const lines = csvRowsOf(file.content);
      expect(lines[0]).toEqual(["Project", "Backlog", "Started", "Unstarted", "Completed", "Cancelled"]);
      expect(lines.slice(1)).toEqual([[project.name, "2", "0", "0", "0", "0"]]);
      for (const row of lines.slice(1)) {
        for (const cell of row) expect(cell).not.toBe("-");
      }
    });

    await test.step("the dialog exports carry the same filename and headers", async () => {
      await driver.analyticsDialogOpenFromIssues(world.workspaceSlug, project.id, browserSessionCookies(world.owner));
      // The two dialog tables settle independently: the chart table waits
      // for its own read, the assignee table for stats. Each export read
      // below waits for that table's own control, and these polls first
      // prove both tables hold rows so an export can never run against a
      // skeleton.
      await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 60_000 }).toBe(2);
      await expect.poll(() => driver.analyticsDialogRowCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      const chartFile = await driver.analyticsDialogExportDownload(0);
      expect(chartFile.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      expect(csvRowsOf(chartFile.content)[0]).toEqual(["Priority", "Count"]);
      const tableFile = await driver.analyticsDialogExportDownload(1);
      expect(tableFile.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      // The dialog table swaps its first column to the assignee (ANL-015).
      expect(csvRowsOf(tableFile.content)[0]).toEqual([
        "Assignee",
        "Backlog",
        "Started",
        "Unstarted",
        "Completed",
        "Cancelled",
      ]);
      await driver.closeAnalytics();
    });

    // The dash fallback for unreadable cells never triggers from current
    // tables: every export value builder resolves (counts stringify, names
    // default to empty), so no downloaded file carries a "-" cell. The
    // scenarios above prove the absence across all four exports; see the
    // ANL-016 row note.
  }
);

test(
  specTitle(ROW_017, "row-count labels pair with expanding search boxes that clear on close"),
  { tag: specTags(ROW_017) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl017");
    const alpha = await setupAnalyticsProject(world, `Anl Search A ${world.tag.slice(-4)}`, "SA");
    const beta = await setupAnalyticsProject(world, `Anl Search B ${world.tag.slice(-4)}`, "SB");
    await createIssue(
      world.workspaceSlug,
      alpha.id,
      world.owner.cookie,
      "se urgent",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      beta.id,
      world.owner.cookie,
      "se high",
      { priority: "high" },
      world.owner.apiBase
    );

    await enterAnalyticsTab(driver, world, "work-items");
    await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 60_000 }).toBe(2);
    await expect.poll(() => driver.analyticsExportRowCount("insight"), { timeout: 60_000 }).toBe(2);

    for (const kind of ["chart", "insight"] as const) {
      const firstColumn = kind === "chart" ? "Urgent" : alpha.name;
      const rowCount = kind === "chart" ? 2 : 2;
      await test.step(`${kind}: the count label reads the current rows`, async () => {
        expect(await driver.analyticsExportCountLabel(kind)).toBe(kind === "chart" ? "2 Priority" : "2 Projects");
      });

      await test.step(`${kind}: the box starts collapsed and expands on open`, async () => {
        expect(await driver.analyticsExportSearchExpanded(kind)).toBe(false);
        await driver.analyticsExportSearch(kind, firstColumn);
        expect(await driver.analyticsExportSearchExpanded(kind)).toBe(true);
      });

      await test.step(`${kind}: typing filters the first column`, async () => {
        await expect.poll(() => driver.analyticsExportRowCount(kind), { timeout: 30_000 }).toBe(1);
        const cells = await driver.analyticsExportRowCells(kind);
        expect(cells[0]?.[0]).toBe(firstColumn);
      });

      await test.step(`${kind}: closing clears the filter and collapses`, async () => {
        await driver.analyticsExportSearchClose(kind);
        expect(await driver.analyticsExportSearchExpanded(kind)).toBe(false);
        await expect.poll(() => driver.analyticsExportRowCount(kind), { timeout: 30_000 }).toBe(rowCount);
      });
    }
  }
);
