// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-375): analytics page shell, tab addressing,
// project scoping, and permissions. Rows ANL-001 (two tabs with the tab id
// in the address, bare-address redirect, unknown tab kept verbatim
// selecting nothing), ANL-002 (section breadcrumb, workspace-naming
// title), ANL-003 (no-project empty view, creation gated to admin/member),
// ANL-004 (project picker search/defaults/summary, selection scopes every
// section), ANL-018 (guests browse and filter with no creation affordance).
// Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend's demo seed (NEWFRONT-265), and creates exactly the projects
// it needs — scenario project names never reuse the workspace name, or the
// drain cannot tell them apart.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  analyticsAdvanceTotalsRaw,
  analyticsProjectCreateStatus,
  analyticsWorkspaceName,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectByName,
  projectsDetails,
  seatFreshMember,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_001 = ["ANL-001"];
const ROW_002 = ["ANL-002"];
const ROW_003 = ["ANL-003"];
const ROW_004 = ["ANL-004"];
const ROW_018 = ["ANL-018"];

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

/** Trailing slashes vary by navigation kind; comparisons ignore them. */
function normalizedPath(path: string): string {
  const withoutQuery = path.split("?", 1)[0] ?? path;
  return withoutQuery.length > 1 ? withoutQuery.replace(/\/+$/, "") : withoutQuery;
}

function totalProjectsOf(payload: unknown): number | null {
  if (typeof payload !== "object" || payload === null) return null;
  const totals = (payload as Record<string, unknown>)["total_projects"];
  if (typeof totals !== "object" || totals === null) return null;
  const count = (totals as Record<string, unknown>)["count"];
  return typeof count === "number" ? count : null;
}

test(
  specTitle(ROW_001, "analytics tabs carry their id in the address and switching moves it"),
  { tag: specTags(ROW_001) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl001");
    const project = await setupAnalyticsProject(world, `Anl One ${world.tag}`, "ANL");

    await enterAnalyticsTab(driver, world, "overview");

    await test.step("two tabs render with the address naming the first", async () => {
      expect(await driver.analyticsTabIds()).toEqual(["overview", "work-items"]);
      expect(await driver.analyticsSelectedTab()).toBe("overview");
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/overview`);
      expect(await driver.analyticsPanelVisible("overview")).toBe(true);
      expect(await driver.analyticsPanelVisible("work-items")).toBe(false);
    });

    await test.step("switching tabs moves the address and the panel", async () => {
      await driver.analyticsSelectTab("work-items");
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/work-items`);
      expect(await driver.analyticsSelectedTab()).toBe("work-items");
      expect(await driver.analyticsPanelVisible("work-items")).toBe(true);
      expect(await driver.analyticsPanelVisible("overview")).toBe(false);

      await driver.analyticsSelectTab("overview");
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/overview`);
      expect(await driver.analyticsSelectedTab()).toBe("overview");
    });

    await test.step("the server holds the project the shell renders over", async () => {
      expect((await projectsDetails(world.owner, world.workspaceSlug)).map((p) => p.name)).toEqual([project.name]);
    });
  }
);

test(
  specTitle(ROW_001, "bare analytics address redirects; unknown tab id is kept verbatim selecting nothing"),
  { tag: specTags(ROW_001) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl001b");
    await setupAnalyticsProject(world, `Anl Bare ${world.tag}`, "ANL");

    await test.step("the bare address redirects to the first tab", async () => {
      await driver.analyticsOpenRoot(world.workspaceSlug, browserSessionCookies(world.owner));
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/overview`);
      expect(await driver.analyticsSelectedTab()).toBe("overview");
    });

    await test.step("an unknown tab id stays in the address with no panel selected", async () => {
      await enterAnalyticsTab(driver, world, "no-such-tab");
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/no-such-tab`);
      expect(await driver.analyticsSelectedTab()).toBeNull();
      expect(await driver.analyticsPanelVisible("overview")).toBe(false);
      expect(await driver.analyticsPanelVisible("work-items")).toBe(false);
      // The strip still offers the two known tabs, and the unknown address
      // survives reading them.
      expect(await driver.analyticsTabIds()).toEqual(["overview", "work-items"]);
      expect(normalizedPath(await driver.analyticsCurrentPath())).toBe(`/${world.workspaceSlug}/analytics/no-such-tab`);
    });
  }
);

test(
  specTitle(ROW_002, "analytics shows the section breadcrumb and a workspace-naming title"),
  { tag: specTags(ROW_002) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl002");
    await setupAnalyticsProject(world, `Anl Crumb ${world.tag}`, "ANL");

    await enterAnalyticsTab(driver, world, "overview");

    await test.step("the app header carries the analytics trail", async () => {
      expect(await driver.analyticsBreadcrumb()).toEqual(["Analytics"]);
    });

    await test.step("the document title names the workspace", async () => {
      const serverName = await analyticsWorkspaceName(world.owner, world.workspaceSlug);
      expect(serverName).toBe(world.workspaceName);
      expect(await driver.analyticsPageTitle()).toBe(`${serverName} - Analytics`);
    });

    await test.step("the breadcrumb and title hold on the second tab", async () => {
      await driver.analyticsSelectTab("work-items");
      expect(await driver.analyticsBreadcrumb()).toEqual(["Analytics"]);
      expect(await driver.analyticsPageTitle()).toBe(`${world.workspaceName} - Analytics`);
    });
  }
);

test(
  specTitle(ROW_003, "empty workspace offers project creation to admins and members"),
  { tag: specTags(ROW_003) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl003");
    expect(await projectsDetails(world.owner, world.workspaceSlug)).toEqual([]);

    await enterAnalyticsTab(driver, world, "overview");

    await test.step("the no-project empty view renders with an enabled creation action", async () => {
      expect(await driver.analyticsTabIds()).toEqual([]);
      const facts = await driver.analyticsEmptyFacts();
      expect(facts?.title).toBe("No project");
      expect(facts?.description.length ?? 0).toBeGreaterThan(0);
      expect(facts?.actionLabel).toBe("Create a project");
      expect(facts?.actionDisabled).toBe(false);
      expect(await driver.analyticsCreateAffordances()).toEqual(["Create a project"]);
    });

    await test.step("a workspace member is offered the same enabled action", async () => {
      const member = await seatAnalyticsUser(world, ROLE.MEMBER, "parity-anl003m");
      await enterAnalyticsTab(driver, world, "overview", member);
      const facts = await driver.analyticsEmptyFacts();
      expect(facts?.actionLabel).toBe("Create a project");
      expect(facts?.actionDisabled).toBe(false);
    });

    await test.step("creating through the dialog leaves the empty view", async () => {
      await enterAnalyticsTab(driver, world, "overview", world.owner);
      await driver.analyticsEmptyCreateProject();
      await expect.poll(() => driver.isCreateProjectDialogVisible(), { timeout: 60_000 }).toBe(true);
      const createdName = `Anl Made ${world.tag}`;
      await driver.fillCreateProjectName(createdName);
      await driver.submitCreateProject();
      await expect.poll(() => driver.analyticsEmptyFacts(), { timeout: 120_000 }).toBeNull();
      await expect.poll(() => driver.analyticsTabIds(), { timeout: 60_000 }).toEqual(["overview", "work-items"]);
      expect(await projectByName(world.owner, world.workspaceSlug, createdName)).toBeDefined();
    });
  }
);

test(
  specTitle(ROW_003.concat(ROW_018), "guests see project creation disabled and the server refuses their create"),
  { tag: specTags(ROW_003.concat(ROW_018)) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl003g");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl003gg");

    await enterAnalyticsTab(driver, world, "overview", guest);

    await test.step("the empty view renders its creation action disabled", async () => {
      const facts = await driver.analyticsEmptyFacts();
      expect(facts?.title).toBe("No project");
      expect(facts?.actionLabel).toBe("Create a project");
      expect(facts?.actionDisabled).toBe(true);
      expect(await driver.analyticsCreateAffordances()).toEqual([]);
      expect(await driver.isCreateProjectDialogVisible()).toBe(false);
    });

    await test.step("the server refuses the guest's project create", async () => {
      expect(
        await analyticsProjectCreateStatus(guest, world.workspaceSlug, {
          name: "Guest Try",
          identifier: parityProjectIdentifier("GT"),
        })
      ).toBe(403);
      expect(await projectsDetails(world.owner, world.workspaceSlug)).toEqual([]);
    });
  }
);

test(
  specTitle(ROW_004, "project picker defaults to everything, searches names and identifiers, summarizes long picks"),
  { tag: specTags(ROW_004) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl004");
    const projects = [];
    for (let i = 0; i < 5; i += 1) {
      projects.push(await setupAnalyticsProject(world, `Anl Pick${i} ${world.tag.slice(-4)}`, `AP${i}`));
    }
    const names = projects.map((p) => p.name);

    await enterAnalyticsTab(driver, world, "overview");

    await test.step("the picker defaults to everything", async () => {
      expect(await driver.analyticsPickerLabel()).toBe("All projects");
      await driver.analyticsPickerOpen();
      expect(await driver.analyticsPickerOptions()).toEqual(expect.arrayContaining(names));
      expect(await driver.analyticsPickerOptions()).toHaveLength(names.length);
      expect(await driver.analyticsPickerChecked()).toEqual([]);
    });

    await test.step("search matches project names", async () => {
      await driver.analyticsPickerSearch("Pick3");
      await expect.poll(() => driver.analyticsPickerOptions(), { timeout: 30_000 }).toEqual([names[3]]);
    });

    await test.step("search matches project identifiers", async () => {
      await driver.analyticsPickerSearch(projects[1]!.identifier.slice(0, 6));
      await expect.poll(() => driver.analyticsPickerOptions(), { timeout: 30_000 }).toEqual([names[1]]);
      await driver.analyticsPickerSearch("");
      await expect.poll(() => driver.analyticsPickerOptions(), { timeout: 30_000 }).toHaveLength(5);
    });

    await test.step("long selections compress into a count summary", async () => {
      for (const name of names.slice(0, 4)) {
        await driver.analyticsPickerToggle(name);
      }
      await expect
        .poll(() => driver.analyticsPickerChecked(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining(names.slice(0, 4)));
      await driver.analyticsPickerClose();
      expect(await driver.analyticsPickerLabel()).toBe("3+ projects");
    });

    await test.step("clearing restores the full workspace", async () => {
      await driver.analyticsPickerOpen();
      for (const name of names.slice(0, 4)) {
        await driver.analyticsPickerToggle(name);
      }
      await expect.poll(() => driver.analyticsPickerChecked(), { timeout: 30_000 }).toEqual([]);
      await driver.analyticsPickerClose();
      expect(await driver.analyticsPickerLabel()).toBe("All projects");
    });

    await test.step("the server lists exactly the picker's projects", async () => {
      expect((await projectsDetails(world.owner, world.workspaceSlug)).map((p) => p.name).sort()).toEqual(
        [...names].sort()
      );
    });
  }
);

test(
  specTitle(ROW_004, "picker selection scopes every analytics read and survives tab switches"),
  { tag: specTags(ROW_004) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl004s");
    const alpha = await setupAnalyticsProject(world, `Anl Alpha ${world.tag.slice(-4)}`, "AA");
    const beta = await setupAnalyticsProject(world, `Anl Beta ${world.tag.slice(-4)}`, "AB");

    await enterAnalyticsTab(driver, world, "overview");
    await driver.analyticsStartRequestSpy();

    await test.step("picking one project filters every later analytics read", async () => {
      await driver.analyticsPickerOpen();
      await driver.analyticsPickerToggle(beta.name);
      await driver.analyticsPickerClose();
      expect(await driver.analyticsPickerLabel()).toBe(beta.name);
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter(
              (r) => new URLSearchParams(r.query).get("project_ids") === beta.id
            ).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      const seen = await driver.analyticsSpyRequests();
      const firstFiltered = seen.findIndex((r) => new URLSearchParams(r.query).get("project_ids") === beta.id);
      expect(firstFiltered).toBeGreaterThanOrEqual(0);
      // Everything from the first filtered read on is narrowed to the pick;
      // earlier captures belong to the pre-selection full-workspace loads.
      for (const request of seen.slice(firstFiltered)) {
        expect(new URLSearchParams(request.query).get("project_ids")).toBe(beta.id);
      }
    });

    await test.step("the scope holds across tab switches", async () => {
      await driver.analyticsSelectTab("work-items");
      expect(await driver.analyticsPickerLabel()).toBe(beta.name);
      await driver.analyticsSelectTab("overview");
      expect(await driver.analyticsPickerLabel()).toBe(beta.name);
      await driver.analyticsStopRequestSpy();
    });

    await test.step("the server narrows totals to the picked project", async () => {
      const full = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "overview");
      expect(full.status).toBe(200);
      expect(totalProjectsOf(full.payload)).toBe(2);
      const scoped = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "overview", [beta.id]);
      expect(scoped.status).toBe(200);
      expect(totalProjectsOf(scoped.payload)).toBe(1);
      const other = await analyticsAdvanceTotalsRaw(world.owner, world.workspaceSlug, "overview", [alpha.id]);
      expect(totalProjectsOf(other.payload)).toBe(1);
    });
  }
);

test(
  specTitle(ROW_018, "guests browse tabs and picker but are offered no creation"),
  { tag: specTags(ROW_018) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl018");
    const alpha = await setupAnalyticsProject(world, `Anl GAlpha ${world.tag.slice(-4)}`, "GA");
    const beta = await setupAnalyticsProject(world, `Anl GBeta ${world.tag.slice(-4)}`, "GB");
    const guest = await seatAnalyticsUser(world, ROLE.GUEST, "parity-anl018g", [alpha.id, beta.id]);

    await enterAnalyticsTab(driver, world, "overview", guest);

    await test.step("tabs, breadcrumb, and title render for the guest", async () => {
      expect(await driver.analyticsTabIds()).toEqual(["overview", "work-items"]);
      expect(await driver.analyticsBreadcrumb()).toEqual(["Analytics"]);
      expect(await driver.analyticsPageTitle()).toBe(`${world.workspaceName} - Analytics`);
    });

    await test.step("the guest filters through the picker over joined projects", async () => {
      expect(await driver.analyticsPickerLabel()).toBe("All projects");
      await driver.analyticsPickerOpen();
      expect(await driver.analyticsPickerOptions()).toEqual(expect.arrayContaining([alpha.name, beta.name]));
      await driver.analyticsPickerToggle(alpha.name);
      await driver.analyticsPickerClose();
      expect(await driver.analyticsPickerLabel()).toBe(alpha.name);
    });

    await test.step("no creation affordance is offered", async () => {
      expect(await driver.analyticsCreateAffordances()).toEqual([]);
      expect(await driver.isCreateProjectDialogVisible()).toBe(false);
    });

    await test.step("the server refuses guest reads and creates alike", async () => {
      // The shell renders over falling-back-to-zero cards; the data reads
      // underneath refuse the guest, as does project creation.
      expect((await analyticsAdvanceTotalsRaw(guest, world.workspaceSlug, "overview")).status).toBe(403);
      expect(
        await analyticsProjectCreateStatus(guest, world.workspaceSlug, {
          name: "Guest Try",
          identifier: parityProjectIdentifier("GT"),
        })
      ).toBe(403);
    });
  }
);
