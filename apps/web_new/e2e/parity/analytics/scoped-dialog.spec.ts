// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-377): analytics scoped dialog. Rows ANL-014
// (work-item analytics open as a dialog scoped to a project, cycle, or
// module from the issue, cycle, and module surfaces; the dialog titles the
// scope, offers a wide-screen toggle on larger screens, spins briefly while
// the scope applies, and clears the scope on close) and ANL-015 (inside the
// dialog the breakdown table swaps its first column from project to
// assignee with avatars, initials, and an unassigned marker; the epic-scope
// heading rewording is unreachable — no shipped surface sets epic scope and
// the table carries no epic branch). Green on apps/web first.
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
  analyticsScopedChartsRaw,
  analyticsScopedStatsRaw,
  analyticsScopedTotalsRaw,
  browserSessionCookies,
  createCycle,
  createIssue,
  createModule,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectStates,
  seatFreshMember,
  serverUploadUserAvatar,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_014 = ["ANL-014"];
const ROW_015 = ["ANL-015"];

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

/** `count` for one totals key, or null when the payload lacks it. */
function countOf(payload: unknown, key: string): number | null {
  if (typeof payload !== "object" || payload === null) return null;
  const field = (payload as Record<string, unknown>)[key];
  if (typeof field !== "object" || field === null) return null;
  const count = (field as Record<string, unknown>)["count"];
  return typeof count === "number" ? count : null;
}

/** Work-items headline keys in card order. */
const WORK_ITEMS_KEYS = [
  "total_work_items",
  "started_work_items",
  "backlog_work_items",
  "un_started_work_items",
  "completed_work_items",
];

test(
  specTitle(ROW_014, "dialog opens project-scoped from issues with title, spinner, wide toggle, and scoped reads"),
  { tag: specTags(ROW_014) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl014");
    const project = await setupAnalyticsProject(world, `Anl Scope ${world.tag.slice(-4)}`, "AS");
    const other = await setupAnalyticsProject(world, `Anl Elsewhere ${world.tag.slice(-4)}`, "AE");
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "scope one", {}, world.owner.apiBase);
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "scope two", {}, world.owner.apiBase);
    await createIssue(world.workspaceSlug, other.id, world.owner.cookie, "scope away", {}, world.owner.apiBase);

    await driver.analyticsStartRequestSpy();
    await driver.analyticsDialogOpenFromIssues(world.workspaceSlug, project.id, browserSessionCookies(world.owner));

    await test.step("the dialog titles the project scope and spins while it applies", async () => {
      // Scope details load async behind the shell; poll for the full title.
      await expect.poll(() => driver.analyticsDialogTitle(), { timeout: 60_000 }).toBe(`Analytics for ${project.name}`);
      expect(await driver.analyticsDialogSpinnerSeen()).toBe(true);
    });

    await test.step("the dialog reads only the scoped project", async () => {
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter((request) =>
              request.path.includes(`/projects/${project.id}/advance-analytics`)
            ).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      const scoped = (await driver.analyticsSpyRequests()).filter((request) =>
        request.path.includes(`/projects/${project.id}/advance-analytics`)
      );
      expect(scoped.length).toBeGreaterThan(0);
      for (const request of scoped) {
        expect(new URLSearchParams(request.query).get("project_ids")).toBe(project.id);
      }
      // Nothing leaks to the sibling project or the bare workspace reads.
      for (const request of await driver.analyticsSpyRequests()) {
        expect(new URLSearchParams(request.query).get("project_ids")).not.toBe(other.id);
      }
    });

    await test.step("dialog headlines match the scoped server totals", async () => {
      const cards = await driver.analyticsDialogHeadlineCards();
      const read = await analyticsScopedTotalsRaw(world.owner, world.workspaceSlug, project.id);
      expect(read.status).toBe(200);
      expect(WORK_ITEMS_KEYS.map((key) => countOf(read.payload, key))).toEqual([2, 0, 2, 0, 0]);
      expect(cards.map((card) => card.value)).toEqual(WORK_ITEMS_KEYS.map((key) => String(countOf(read.payload, key))));
    });

    await test.step("the scoped chart and stats reads answer for the dialog", async () => {
      const charts = await analyticsScopedChartsRaw(world.owner, world.workspaceSlug, project.id, "work-items");
      expect(charts.status).toBe(200);
      const stats = await analyticsScopedStatsRaw(world.owner, world.workspaceSlug, project.id);
      expect(stats.status).toBe(200);
      expect(Array.isArray(stats.payload)).toBe(true);
    });

    await test.step("the wide toggle resizes on larger screens", async () => {
      expect(await driver.analyticsDialogWideToggleVisible()).toBe(true);
      const narrow = await driver.analyticsDialogWidth();
      await driver.analyticsDialogToggleWide();
      const wide = await driver.analyticsDialogWidth();
      expect(wide).toBeGreaterThan(narrow);
      await driver.analyticsDialogToggleWide();
      expect(await driver.analyticsDialogWidth()).toBeLessThan(wide);
    });

    await test.step("closing clears the scope so the workspace view is unaffected", async () => {
      await driver.closeAnalytics();
      await driver.analyticsStopRequestSpy();
      await enterAnalyticsTab(driver, world, "work-items");
      expect(await driver.analyticsPickerLabel()).toBe("All projects");
      const cards = await driver.analyticsWorkItemsCards();
      expect(cards.map((card) => card.value)).toEqual(["3", "0", "3", "0", "0"]);
    });
  }
);

test(
  specTitle(ROW_014, "dialog titles cycle and module scopes and narrows their reads"),
  { tag: specTags(ROW_014) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl014c");
    const project = await setupAnalyticsProject(world, `Anl Scoped ${world.tag.slice(-4)}`, "AC");
    const cycle = await createCycle(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "Scope Cycle",
      world.owner.apiBase
    );
    const module = await createModule(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "Scope Module",
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "scoped one", {}, world.owner.apiBase);

    await test.step("the cycle surface scopes the dialog to the cycle", async () => {
      await driver.analyticsStartRequestSpy();
      await driver.analyticsDialogOpenFromCycle(
        world.workspaceSlug,
        project.id,
        cycle.id,
        browserSessionCookies(world.owner)
      );
      await expect
        .poll(() => driver.analyticsDialogTitle(), { timeout: 60_000 })
        .toBe(`Analytics for ${project.name} in ${cycle.name}`);
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter(
              (request) => new URLSearchParams(request.query).get("cycle_id") === cycle.id
            ).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      const scoped = await analyticsScopedTotalsRaw(world.owner, world.workspaceSlug, project.id, {
        cycle_id: cycle.id,
      });
      expect(scoped.status).toBe(200);
      await driver.closeAnalytics();
      await driver.analyticsStopRequestSpy();
    });

    await test.step("the module surface scopes the dialog to the module", async () => {
      await driver.analyticsStartRequestSpy();
      await driver.analyticsDialogOpenFromModule(
        world.workspaceSlug,
        project.id,
        module.id,
        browserSessionCookies(world.owner)
      );
      // The oracle's title template leaves an empty cycle fragment behind,
      // so its module title carries a double space. Spec §13 declares one
      // space as the intended wording and the new build implements it;
      // this scenario records the divergence rather than the typo.
      const moduleTitle =
        driver.target === "web_new"
          ? `Analytics for ${project.name} in ${module.name}`
          : `Analytics for ${project.name}  in ${module.name}`;
      await expect.poll(() => driver.analyticsDialogTitle(), { timeout: 60_000 }).toBe(moduleTitle);
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter(
              (request) => new URLSearchParams(request.query).get("module_id") === module.id
            ).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      const scoped = await analyticsScopedTotalsRaw(world.owner, world.workspaceSlug, project.id, {
        module_id: module.id,
      });
      expect(scoped.status).toBe(200);
      await driver.closeAnalytics();
      await driver.analyticsStopRequestSpy();
    });
  }
);

test(
  specTitle(ROW_014, "guests are offered no dialog trigger on desktop surfaces"),
  { tag: specTags(ROW_014) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl014g");
    const project = await setupAnalyticsProject(world, `Anl Guest ${world.tag.slice(-4)}`, "AG");
    const cycle = await createCycle(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "Guest Cycle",
      world.owner.apiBase
    );
    const module = await createModule(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "Guest Module",
      world.owner.apiBase
    );
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl014gg", [project.id]);

    // The desktop headers gate the trigger on issue-creation rights, which
    // guests lack — see the ANL-014 row note.
    for (const path of [
      `/${world.workspaceSlug}/projects/${project.id}/issues`,
      `/${world.workspaceSlug}/projects/${project.id}/cycles/${cycle.id}`,
      `/${world.workspaceSlug}/projects/${project.id}/modules/${module.id}`,
    ]) {
      await driver.openAuthenticated(path, browserSessionCookies(guest));
      await expect.poll(() => driver.analyticsSurfaceTriggerCount(), { timeout: 60_000 }).toBe(0);
    }
  }
);

test(
  specTitle(ROW_015, "dialog table swaps to assignees with avatar, initial, and unassigned rows"),
  { tag: specTags(ROW_015) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl015");
    const project = await setupAnalyticsProject(world, `Anl Assignee ${world.tag.slice(-4)}`, "AN");
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    if (states.length === 0) throw new Error("[parity] seeded project has no states.");
    const pictured = await seatAnalyticsUser(world, ROLE.MEMBER, "parity-anl015p", [project.id]);
    const plain = await seatAnalyticsUser(world, ROLE.MEMBER, "parity-anl015q", [project.id]);
    // A one-pixel PNG; the bytes only need to satisfy the image upload.
    const pixel = Uint8Array.from([
      137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196,
      137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 0, 0, 0, 2, 0, 1, 225, 27, 205, 133, 0, 0, 0, 0, 73, 69, 78,
      68, 174, 66, 96, 130,
    ]);
    await serverUploadUserAvatar(pictured, { name: `anl015-${world.tag}.png`, mime: "image/png", bytes: pixel });
    const first = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "asg pictured",
      {},
      world.owner.apiBase
    );
    const second = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "asg plain",
      {},
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "asg open", {}, world.owner.apiBase);
    for (const [issueId, userId] of [
      [first.id, pictured.userId],
      [second.id, plain.userId],
    ] as const) {
      const res = await fetch(
        `${world.owner.apiBase}/api/workspaces/${world.workspaceSlug}/projects/${project.id}/issues/${issueId}/`,
        {
          method: "PATCH",
          headers: { "content-type": "application/json", cookie: world.owner.cookie },
          body: JSON.stringify({ assignee_ids: [userId] }),
        }
      );
      if (!res.ok) throw new Error(`[parity] assignee seed failed with HTTP ${res.status}.`);
    }

    await driver.analyticsDialogOpenFromIssues(world.workspaceSlug, project.id, browserSessionCookies(world.owner));

    // Settle polls row count atomically (multi-call reads race row-count
    // shrinkage), then read each surface once.
    await expect.poll(() => driver.analyticsDialogRowCount(), { timeout: 60_000 }).toBe(3);

    await test.step("the first column swaps from project to assignee", async () => {
      expect(await driver.analyticsDialogTableHeaders()).toEqual([
        "Assignee",
        "Backlog",
        "Started",
        "Unstarted",
        "Completed",
        "Cancelled",
      ]);
    });

    await test.step("avatar, initial, and unassigned rows render their identities", async () => {
      const facts = await driver.analyticsDialogAssigneeFacts();
      const byText = new Map(facts.map((fact) => [fact.text, fact]));
      const picturedFact = [...byText.values()].find((fact) => fact.avatar);
      expect(picturedFact).toBeDefined();
      expect(picturedFact!.initial).toBe(false);
      expect(picturedFact!.unassigned).toBe(false);
      const plainFact = [...byText.values()].find((fact) => fact.initial);
      expect(plainFact).toBeDefined();
      expect(plainFact!.text.length).toBeGreaterThan(0);
      expect(byText.get("Unassigned")).toEqual({ text: "Unassigned", avatar: false, initial: false, unassigned: true });
    });

    await test.step("row counts match the scoped server stats", async () => {
      const read = await analyticsScopedStatsRaw(world.owner, world.workspaceSlug, project.id);
      expect(read.status).toBe(200);
      const rows = Array.isArray(read.payload) ? (read.payload as Record<string, unknown>[]) : [];
      expect(rows).toHaveLength(3);
      // Identity texts (name spans) join to the server rows; body cells
      // carry the counts in the same row order.
      const facts = await driver.analyticsDialogAssigneeFacts();
      const cells = await driver.analyticsDialogTableCells();
      expect(cells).toHaveLength(3);
      facts.forEach((fact, index) => {
        const server = rows.find((row) =>
          fact.unassigned ? row["display_name"] === null : row["display_name"] === fact.text
        );
        expect(server).toBeDefined();
        expect(cells[index]?.slice(1)).toEqual(
          [
            "backlog_work_items",
            "started_work_items",
            "un_started_work_items",
            "completed_work_items",
            "cancelled_work_items",
          ].map((key) => String(server![key]))
        );
      });
      await driver.closeAnalytics();
    });

    await test.step("the reachable dialog keeps work-items headings (epic rewording unreachable)", async () => {
      // No shipped surface sets epic scope — the dialog's only callers pass
      // project scope (the issues filter defaults to it) — and the table
      // carries no epic heading branch, so the headings above are the only
      // reachable wording. See the ANL-015 row note.
      await driver.analyticsDialogOpenFromIssues(world.workspaceSlug, project.id, browserSessionCookies(world.owner));
      await expect.poll(() => driver.analyticsDialogRowCount(), { timeout: 60_000 }).toBe(3);
      expect(await driver.analyticsDialogTableHeaders()).toEqual([
        "Assignee",
        "Backlog",
        "Started",
        "Unstarted",
        "Completed",
        "Cancelled",
      ]);
      await driver.closeAnalytics();
    });
  }
);
