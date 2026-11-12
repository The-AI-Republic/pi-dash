// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-376): analytics work-items tab headline
// section. Rows ANL-008 (headline totals split by workflow progress; the
// epic-scope label rewording is unreachable from any shipped surface, so the
// reachable scoped dialog keeps the work-items wording) and ANL-009 (created
// versus resolved smoothed area chart with calendar axis labels and a
// legend; loader during fetch; guidance empty view when the series read
// fails). Green on apps/web first.
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
  analyticsAdvanceChartsRaw,
  analyticsAdvanceTotalsRaw,
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

const ROW_008 = ["ANL-008"];
const ROW_009 = ["ANL-009"];

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

/** `count` for one totals key, or null when the payload lacks it. */
function countOf(payload: unknown, key: string): number | null {
  if (typeof payload !== "object" || payload === null) return null;
  const field = (payload as Record<string, unknown>)[key];
  if (typeof field !== "object" || field === null) return null;
  const count = (field as Record<string, unknown>)["count"];
  return typeof count === "number" ? count : null;
}

/** Month rows of a work-items-graph payload (`type=work-items` reads an object). */
function chartDataOf(payload: unknown): Array<{ key: string; created: number; completed: number }> {
  if (typeof payload !== "object" || payload === null) return [];
  const data = (payload as Record<string, unknown>)["data"];
  if (!Array.isArray(data)) return [];
  const rows: Array<{ key: string; created: number; completed: number }> = [];
  for (const row of data) {
    if (typeof row !== "object" || row === null) continue;
    const record = row as Record<string, unknown>;
    if (typeof record["key"] !== "string") continue;
    if (typeof record["created_issues"] !== "number" || typeof record["completed_issues"] !== "number") continue;
    rows.push({ key: record["key"], created: record["created_issues"], completed: record["completed_issues"] });
  }
  return rows;
}

/** Calendar label for a month key (`2026-10-01` renders `Oct 01, 2026`). */
function formatChartMonth(key: string): string {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const [year, month, day] = key.split("-");
  const index = Number(month ?? "0") - 1;
  return `${months[index] ?? month} ${day}, ${year}`;
}

/** Work-items headline keys in card order. */
const WORK_ITEMS_KEYS = [
  "total_work_items",
  "started_work_items",
  "backlog_work_items",
  "un_started_work_items",
  "completed_work_items",
];

const WORK_ITEMS_LABELS = [
  "Total Work items",
  "Started Work items",
  "Backlog Work items",
  "Unstarted Work items",
  "Completed Work items",
];

const GUIDANCE_TEXT =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

test(
  specTitle(ROW_008, "work-items headline cards match server totals per workflow group behind placeholders"),
  { tag: specTags(ROW_008) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl008");
    const project = await setupAnalyticsProject(world, `Anl Groups ${world.tag.slice(-4)}`, "AG");
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const stateIdOf = (group: string): string => {
      const id = states.find((s) => s.group === group)?.id;
      if (!id) throw new Error(`[parity] seeded project has no ${group} state.`);
      return id;
    };
    const started = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "groups started",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "groups backlog", {}, world.owner.apiBase);
    const unstarted = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "groups unstarted",
      {},
      world.owner.apiBase
    );
    const completed = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "groups completed",
      {},
      world.owner.apiBase
    );
    await moveSeededIssue(world, project.id, started.id, stateIdOf("started"));
    await moveSeededIssue(world, project.id, unstarted.id, stateIdOf("unstarted"));
    await moveSeededIssue(world, project.id, completed.id, stateIdOf("completed"));

    await test.step("placeholders cover the grid while the totals read flies", async () => {
      await driver.analyticsTotalsStall("work-items");
      await enterAnalyticsTab(driver, world, "work-items");
      await expect.poll(() => driver.analyticsHeadlinePlaceholders(), { timeout: 60_000 }).toBe(5);
      await driver.analyticsTotalsRelease();
    });

    await test.step("all five cards render with values matching the current scope", async () => {
      const cards = await driver.analyticsWorkItemsCards();
      expect(cards.map((card) => card.label)).toEqual(WORK_ITEMS_LABELS);
      const read = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "work-items");
      expect(read.status).toBe(200);
      expect(WORK_ITEMS_KEYS.map((key) => countOf(read.payload, key))).toEqual([4, 1, 1, 1, 1]);
      expect(cards.map((card) => card.value)).toEqual(WORK_ITEMS_KEYS.map((key) => String(countOf(read.payload, key))));
    });
  }
);

test(
  specTitle(ROW_008, "headline wording stays work-items inside the reachable scoped dialog"),
  { tag: specTags(ROW_008) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    // The epic-scope rewording branch exists but no shipped surface sets epic
    // scope: no route supplies an epic id and the only dialog caller uses the
    // default project scope. This scenario proves the actual behavior — the
    // reachable dialog keeps the work-items wording — and the ANL-008 row
    // records the dead branch (see the row note).
    const world = await setupAnalyticsWorld("parity-anl008d");
    const project = await setupAnalyticsProject(world, `Anl Dialog ${world.tag.slice(-4)}`, "AD");
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const doneId = states.find((s) => s.group === "completed")?.id;
    if (!doneId) throw new Error("[parity] seeded project has no completed state.");
    const first = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "dialog one",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "dialog two", {}, world.owner.apiBase);
    await moveSeededIssue(world, project.id, first.id, doneId);

    await enterAnalyticsTab(driver, world, "work-items");
    const tabCards = await driver.analyticsWorkItemsCards();
    expect(tabCards.map((card) => card.label)).toEqual(WORK_ITEMS_LABELS);

    await test.step("the issues-header dialog shows the same work-items wording", async () => {
      await driver.openProjectIssues(world.workspaceSlug, project.id);
      await driver.openAnalytics();
      const dialogCards = await driver.analyticsDialogHeadlineCards();
      expect(dialogCards.map((card) => card.label)).toEqual(WORK_ITEMS_LABELS);
      // A single-project workspace scopes the dialog to the whole workspace.
      expect(dialogCards.map((card) => card.value)).toEqual(tabCards.map((card) => card.value));
      await driver.closeAnalytics();
    });

    await test.step("the server totals agree with both surfaces", async () => {
      const read = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "work-items");
      expect(read.status).toBe(200);
      expect(WORK_ITEMS_KEYS.map((key) => countOf(read.payload, key))).toEqual([2, 0, 1, 0, 1]);
      expect(tabCards.map((card) => card.value)).toEqual(
        WORK_ITEMS_KEYS.map((key) => String(countOf(read.payload, key)))
      );
    });
  }
);

test(
  specTitle(ROW_009, "created-vs-resolved chart matches the server series behind a loader"),
  { tag: specTags(ROW_009) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl009");
    const project = await setupAnalyticsProject(world, `Anl Series ${world.tag.slice(-4)}`, "AS");
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const doneId = states.find((s) => s.group === "completed")?.id;
    if (!doneId) throw new Error("[parity] seeded project has no completed state.");
    const first = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "series one",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "series two", {}, world.owner.apiBase);
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "series three", {}, world.owner.apiBase);
    await moveSeededIssue(world, project.id, first.id, doneId);

    await test.step("a loader covers the section while the graph read flies", async () => {
      await driver.analyticsChartsStall("work-items");
      await enterAnalyticsTab(driver, world, "work-items");
      await expect.poll(() => driver.analyticsCreatedResolvedLoading(), { timeout: 60_000 }).toBe(true);
      await driver.analyticsChartsRelease();
    });

    await test.step("both series render with calendar labels and a legend", async () => {
      const read = await analyticsAdvanceChartsRaw(world.owner, world.workspaceSlug, "work-items");
      expect(read.status).toBe(200);
      const rows = chartDataOf(read.payload);
      expect(rows.length).toBeGreaterThan(0);
      expect(rows.reduce((sum, row) => sum + row.created, 0)).toBe(3);
      expect(rows.reduce((sum, row) => sum + row.completed, 0)).toBe(1);
      const facts = await driver.analyticsCreatedResolvedFacts();
      expect(facts.guidance).toBeNull();
      expect(facts.legend).toEqual(["Resolved", "Created"]);
      expect(facts.series).toBe(2);
      expect(facts.xLabels).toEqual(rows.map((row) => formatChartMonth(row.key)));
    });
  }
);

test(
  specTitle(ROW_009, "created-vs-resolved with no readable series shows the guidance view"),
  { tag: specTags(ROW_009) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl009g");
    const project = await setupAnalyticsProject(world, `Anl Ungraphed ${world.tag.slice(-4)}`, "AU");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl009gg", [project.id]);

    await test.step("the server refuses the guest's graph read", async () => {
      expect((await analyticsAdvanceChartsRaw(guest, world.workspaceSlug, "work-items")).status).toBe(403);
    });

    await test.step("the section shows the guidance view instead of the chart", async () => {
      await enterAnalyticsTab(driver, world, "work-items", guest);
      // The failed read retries in the background, so the loader flashes
      // between guidance renders; poll for the guidance text itself.
      await expect
        .poll(() => driver.analyticsSectionGuidance("Created vs Resolved"), { timeout: 60_000 })
        .toBe(GUIDANCE_TEXT);
      const facts = await driver.analyticsCreatedResolvedFacts();
      expect(facts.legend).toEqual([]);
      expect(facts.xLabels).toEqual([]);
      expect(facts.series).toBe(0);
    });
  }
);
