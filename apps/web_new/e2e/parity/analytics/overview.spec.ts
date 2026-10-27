// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-376): analytics overview tab. Rows ANL-005
// (headline totals grid: people overall and per role, projects, work items,
// cycles, intake, agent token usage; loading placeholders; missing metrics
// read as zero), ANL-006 (per-project radar chart plus the written
// per-dimension summary; skeleton during load; headers-only section when the
// graph read fails), ANL-007 (active-projects list with a completion badge
// per project, long names shortened with the full name on hover, stale
// entries skipped). Green on apps/web first.
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
  analyticsProjectStatsRaw,
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

const ROW_005 = ["ANL-005"];
const ROW_006 = ["ANL-006"];
const ROW_007 = ["ANL-007"];

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

/** Dimension rows of a projects-graph payload (`type=projects` reads a list). */
function chartRowsOf(payload: unknown): Array<{ key: string; name: string; count: number }> {
  if (!Array.isArray(payload)) return [];
  const rows: Array<{ key: string; name: string; count: number }> = [];
  for (const row of payload) {
    if (typeof row !== "object" || row === null) continue;
    const record = row as Record<string, unknown>;
    if (typeof record["key"] !== "string" || typeof record["name"] !== "string") continue;
    if (typeof record["count"] !== "number") continue;
    rows.push({ key: record["key"], name: record["name"], count: record["count"] });
  }
  return rows;
}

/** Per-project rows of a project-stats payload. */
function statRowsOf(payload: unknown): Array<{ id: string; total: number; completed: number }> {
  if (!Array.isArray(payload)) return [];
  const rows: Array<{ id: string; total: number; completed: number }> = [];
  for (const row of payload) {
    if (typeof row !== "object" || row === null) continue;
    const record = row as Record<string, unknown>;
    if (typeof record["id"] !== "string") continue;
    if (typeof record["total_issues"] !== "number" || typeof record["completed_issues"] !== "number") continue;
    rows.push({ id: record["id"], total: record["total_issues"], completed: record["completed_issues"] });
  }
  return rows;
}

/** Overview headline keys in card order. */
const OVERVIEW_KEYS = [
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
];

test(
  specTitle(ROW_005, "overview headline cards match server totals, load behind placeholders, zero out for guests"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl005");
    const project = await setupAnalyticsProject(world, `Anl Totals ${world.tag.slice(-4)}`, "AT");
    await seatAnalyticsUser(world, ROLE.MEMBER, "parity-anl005m");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl005g", [project.id]);
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const doneId = states.find((s) => s.group === "completed")?.id;
    const first = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "totals one",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "totals two", {}, world.owner.apiBase);
    if (!doneId) throw new Error("[parity] seeded project has no completed state.");
    await moveSeededIssue(world, project.id, first.id, doneId);

    await test.step("placeholders cover the grid while the totals read flies", async () => {
      await driver.analyticsTotalsStall("overview");
      await enterAnalyticsTab(driver, world, "overview");
      await expect.poll(() => driver.analyticsHeadlinePlaceholders(), { timeout: 60_000 }).toBe(11);
      await driver.analyticsTotalsRelease();
    });

    await test.step("every card value matches the server totals", async () => {
      const cards = await driver.analyticsOverviewCards();
      expect(cards).toHaveLength(11);
      const read = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "overview");
      expect(read.status).toBe(200);
      expect(OVERVIEW_KEYS.map((key) => countOf(read.payload, key))).toEqual([3, 1, 1, 1, 1, 2, 0, 0, 0, 0, 0]);
      expect(cards.map((card) => card.value)).toEqual(OVERVIEW_KEYS.map((key) => String(countOf(read.payload, key))));
    });

    await test.step("missing metrics read as zero for the refused guest", async () => {
      const read = await analyticsAdvanceTotalsRaw(guest, world.workspaceSlug, "overview");
      expect(read.status).toBe(403);
      await enterAnalyticsTab(driver, world, "overview", guest);
      const cards = await driver.analyticsOverviewCards();
      expect(cards).toHaveLength(11);
      expect(cards.map((card) => card.value)).toEqual(Array(11).fill("0"));
    });
  }
);

test(
  specTitle(ROW_005, "bug: NEWFRONT-468 overview headline cards render raw i18n keys as labels"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    // The values above are correct; the labels are not. The overview entity
    // names resolve through nested translation keys with no English entry,
    // so the app shows the keys verbatim. Intended labels live in
    // NEWFRONT-468 and the ANL-005 row; the new frontend implements those.
    const world = await setupAnalyticsWorld("parity-anl005b");
    await setupAnalyticsProject(world, `Anl Labels ${world.tag.slice(-4)}`, "AL");

    await enterAnalyticsTab(driver, world, "overview");

    const cards = await driver.analyticsOverviewCards();
    // Spec §5 declares the eight human labels the intended wording and
    // the new build implements them; the oracle pins the raw keys. This
    // scenario records the divergence rather than a second copy of the bug.
    const expected =
      driver.target === "web_new"
        ? [
            "Total Users",
            "Total Admins",
            "Total Members",
            "Total Guests",
            "Total Projects",
            "Total Work items",
            "Total Cycles",
            "Total Intake",
          ]
        : [
            "Total common.users",
            "Total common.admins",
            "Total common.members",
            "Total common.guests",
            "Total common.projects",
            "Total common.work_items",
            "Total common.cycles",
            "Total sidebar.intake",
          ];
    expect(cards.map((card) => card.label)).toEqual([
      ...expected,
      "Agent input tokens",
      "Agent output tokens",
      "Agent total tokens",
    ]);
  }
);

test(
  specTitle(ROW_006, "radar chart and written summary agree with the server graph behind a skeleton"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl006");
    const project = await setupAnalyticsProject(world, `Anl Radar ${world.tag.slice(-4)}`, "AR");
    await seatAnalyticsUser(world, ROLE.MEMBER, "parity-anl006m");
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "radar one", {}, world.owner.apiBase);
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "radar two", {}, world.owner.apiBase);

    await test.step("a skeleton covers the section while the graph read flies", async () => {
      await driver.analyticsChartsStall("projects");
      await enterAnalyticsTab(driver, world, "overview");
      await expect.poll(() => driver.analyticsRadarLoading(), { timeout: 60_000 }).toBe(true);
      await driver.analyticsChartsRelease();
    });

    await test.step("chart and summary agree with the server on dimensions and counts", async () => {
      const read = await analyticsAdvanceChartsRaw(world.owner, world.workspaceSlug, "projects");
      expect(read.status).toBe(200);
      const rows = chartRowsOf(read.payload);
      expect(rows.map((row) => row.name)).toEqual([
        "Work Items",
        "Cycles",
        "Modules",
        "Intake",
        "Members",
        "Pages",
        "Views",
      ]);
      expect(rows.find((row) => row.key === "work_items")?.count).toBe(2);
      const summary = await driver.analyticsRadarSummary();
      expect(summary).toEqual(rows.map((row) => ({ dimension: row.name, count: String(row.count) })));
      const facts = await driver.analyticsRadarFacts();
      expect(facts.plotted).toBe(true);
      expect(facts.dimensions).toEqual(rows.map((row) => row.name));
      expect(await driver.analyticsSectionGuidance("Project Insights")).toBeNull();
    });

    await test.step("a member with nothing to plot still gets every zeroed dimension", async () => {
      const empty = await setupAnalyticsWorld("parity-anl006e");
      await setupAnalyticsProject(empty, `Anl Barren ${empty.tag.slice(-4)}`, "AB");
      await enterAnalyticsTab(driver, empty, "overview");
      // The graph read always carries all seven dimensions, so the empty
      // panel's no-rows condition never holds for members: the section plots
      // zeroed dimensions instead (see the ANL-006 row note). The summary
      // read settles the section first; the instant facts read follows.
      const summary = await driver.analyticsRadarSummary();
      expect(summary).toHaveLength(7);
      const facts = await driver.analyticsRadarFacts();
      expect(facts.plotted).toBe(true);
      expect(facts.dimensions).toHaveLength(7);
      expect(summary.find((row) => row.dimension === "Work Items")?.count).toBe("0");
      expect(await driver.analyticsSectionGuidance("Project Insights")).toBeNull();
    });
  }
);

test(
  specTitle(ROW_006, "radar with no readable graph shows headers without a graphic or guidance"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl006g");
    const project = await setupAnalyticsProject(world, `Anl Dim ${world.tag.slice(-4)}`, "AD");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl006gg", [project.id]);

    await test.step("the server refuses the guest's graph read", async () => {
      expect((await analyticsAdvanceChartsRaw(guest, world.workspaceSlug, "projects")).status).toBe(403);
    });

    await test.step("the section keeps its headers but plots nothing", async () => {
      await enterAnalyticsTab(driver, world, "overview", guest);
      // The summary read settles the section first; the instant facts read
      // follows, so neither can catch the skeleton mid-mount.
      expect(await driver.analyticsRadarSummary()).toEqual([]);
      const facts = await driver.analyticsRadarFacts();
      expect(facts.plotted).toBe(false);
      expect(facts.dimensions).toEqual([]);
      // A failed read is not an empty list, so the guidance panel's
      // condition never holds either: no guidance renders (row note).
      expect(await driver.analyticsSectionGuidance("Project Insights")).toBeNull();
    });
  }
);

test(
  specTitle(ROW_007, "active-projects list badges tones truncation hover and stale skips"),
  { tag: specTags(ROW_007) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl007");
    const longName = `Anl Active With An Extremely Long Project Name That Must Shorten ${world.tag.slice(-4)}`;
    const full = await setupAnalyticsProject(world, longName, "AF");
    const half = await setupAnalyticsProject(world, `Anl Half ${world.tag.slice(-4)}`, "AH");
    const none = await setupAnalyticsProject(world, `Anl None ${world.tag.slice(-4)}`, "AN");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl007g", [half.id]);

    const seedCompletion = async (projectId: string, total: number, completed: number, prefix: string) => {
      const states = await projectStates(world.workspaceSlug, projectId, world.owner.cookie, world.owner.apiBase);
      const doneId = states.find((s) => s.group === "completed")?.id;
      if (!doneId) throw new Error("[parity] seeded project has no completed state.");
      for (let i = 0; i < total; i += 1) {
        const issue = await createIssue(
          world.workspaceSlug,
          projectId,
          world.owner.cookie,
          `${prefix} ${i}`,
          {},
          world.owner.apiBase
        );
        if (i < completed) await moveSeededIssue(world, projectId, issue.id, doneId);
      }
    };
    await seedCompletion(full.id, 2, 2, "full");
    await seedCompletion(half.id, 2, 1, "half");
    await seedCompletion(none.id, 1, 0, "none");

    await test.step("placeholder rows cover the list while the stats read flies", async () => {
      await driver.analyticsProjectStatsStall();
      await enterAnalyticsTab(driver, world, "overview");
      await expect.poll(() => driver.analyticsActiveProjectsLoading(), { timeout: 60_000 }).toBe(true);
      await driver.analyticsProjectStatsRelease();
    });

    await test.step("badges match server math and flip tone above halfway", async () => {
      const read = await analyticsProjectStatsRaw(world.owner, world.workspaceSlug);
      expect(read.status).toBe(200);
      const stats = statRowsOf(read.payload);
      expect(stats).toHaveLength(3);
      const rows = await driver.analyticsActiveProjects();
      expect(rows).toHaveLength(3);
      for (const row of rows) {
        const stat = stats.find((s) => {
          const known = [full, half, none].find((p) => p.name === row.name);
          return known !== undefined && s.id === known.id;
        });
        expect(stat).toBeDefined();
        const expected = stat!.total === 0 ? 0 : Math.round((stat!.completed / stat!.total) * 100);
        expect(row.badge).toBe(`${expected}%`);
        expect(row.tone).toBe(expected > 50 ? "success" : "danger");
      }
      const byName = new Map(rows.map((row) => [row.name, row]));
      expect(byName.get(longName)?.badge).toBe("100%");
      expect(byName.get(longName)?.tone).toBe("success");
      expect(byName.get(half.name)?.badge).toBe("50%");
      expect(byName.get(half.name)?.tone).toBe("danger");
      expect(byName.get(none.name)?.badge).toBe("0%");
    });

    await test.step("long names shorten with the full form on hover", async () => {
      const rows = await driver.analyticsActiveProjects();
      const byName = new Map(rows.map((row) => [row.name, row]));
      expect(byName.get(longName)?.truncated).toBe(true);
      expect(byName.get(half.name)?.truncated).toBe(false);
      expect(await driver.analyticsActiveProjectTooltip(longName)).toBe(longName);
    });

    await test.step("entries the viewer cannot resolve are skipped", async () => {
      // The stats read covers every workspace project while the list only
      // renders what resolves in the viewer's project store.
      const read = await analyticsProjectStatsRaw(guest, world.workspaceSlug);
      expect(read.status).toBe(200);
      expect(statRowsOf(read.payload)).toHaveLength(3);
      await enterAnalyticsTab(driver, world, "overview", guest);
      const rows = await driver.analyticsActiveProjects();
      expect(rows.map((row) => row.name)).toEqual([half.name]);
      expect(rows[0]?.badge).toBe("50%");
    });
  }
);
