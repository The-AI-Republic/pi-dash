// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-310): module detail — breadcrumb trail with a
// searchable module switcher, the work-item count badge, tab title, the
// unknown-module empty state, detail work-item layouts with display
// options and filters, adding work items, the analytics dialog with the
// collapsible sidebar, sidebar fields, the progress chart with breakdowns
// that filter the list, and module links.
// Rows: MOD-029–MOD-035.
import { test, expect } from "../fixtures";
import {
  archivesArchiveModule,
  parityApiBase,
  parityProjectIdentifier,
  serverAddIssuesToModule,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCreateEstimate,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverEnsureProjectGuest,
  serverMe,
  serverModuleIssueIds,
  serverModulesDetailGet,
  serverModulesDetailLinks,
  serverModulesDetailPatch,
  serverModulesDetailUserProperties,
  serverPatchIssue,
  serverPatchModuleUserProperties,
  serverRequestStatus,
  serverSetProjectEstimate,
  serverStates,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

test.use({ viewport: { width: 1600, height: 900 } });

/** Module dates that read ACTIVE on the seeded stack (today is 2026-10-10). */
const ACTIVE_START = "2026-09-01";
const ACTIVE_END = "2026-12-31";
/** Module dates that read NOT-STARTED (a future start hides the chart). */
const FUTURE_START = "2026-11-01";
const FUTURE_END = "2026-11-30";
/** Rich-expression store form ({and:[{prop__in:value}]}); priority=urgent matches no fixture. */
const MATCH_NOTHING = { and: [{ priority__in: "urgent" }] };

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Second seeded user (for member options, which name the viewer "You"). */
function requireMention(seed: ParitySeedFacts): { id: string; displayName: string } {
  if (!seed.mentionMember) throw new Error("[parity] seed has no mention member; rerun parity-up.sh.");
  return { id: seed.mentionMember.id, displayName: seed.mentionMember.displayName };
}

/** Project MEMBER role id (archives precedent). */
const ROLE_MEMBER = 15;

/** Scratch project with the modules surface enabled; resolves with its id. */
async function createModuleProject(
  seed: ParitySeedFacts,
  name: string,
  session: string,
  flags: { cycleView?: boolean; moduleView?: boolean } = { cycleView: true, moduleView: true }
): Promise<string> {
  return serverCreateProjectWithFlags(seed.workspaceSlug, name, parityProjectIdentifier("N310"), flags, session);
}

/** Create one issue in a named state and resolve with its id. */
async function createStateIssue(
  seed: ParitySeedFacts,
  projectId: string,
  name: string,
  stateName: string,
  session: string
): Promise<string> {
  const states = await serverStates(seed.workspaceSlug, projectId, session);
  const stateId = states.find((row) => row.name.toLowerCase() === stateName)?.id;
  if (stateId === undefined) throw new Error(`[parity] project has no ${stateName} state.`);
  const id = await serverCreateIssue(seed.workspaceSlug, projectId, session, name);
  await serverPatchIssue(seed.workspaceSlug, projectId, id, { state: stateId }, session);
  return id;
}

/**
 * Open a module detail page with cookie auth and wait for its sidebar to
 * name the module, then for the layout marker to settle (the header
 * switcher renders before the stored selection applies). Cold entry
 * settles: the detail route fetches on mount.
 */
async function openDetail(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  moduleId: string,
  moduleName: string,
  session: string
): Promise<void> {
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${projectId}/modules/${moduleId}`,
    sessionBrowserCookies(session)
  );
  await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(moduleName);
  await driver.layoutsActiveLayout();
}

/** Most recent toast as one string, polled until non-empty. */
async function toastText(driver: ParityDriver): Promise<string> {
  let text = "";
  await expect
    .poll(
      async () => {
        const toast = await driver.rulesLastToast();
        text = toast ? `${toast.title} ${toast.message}` : "";
        return text;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return text;
}

/** Current path without a trailing slash (client-side hops add one). */
async function currentPathClean(driver: ParityDriver): Promise<string> {
  return (await driver.currentPath()).replace(/\/$/, "");
}

/**
 * Wait until no toast shows (passes immediately when none shows), so a
 * chained action's notice never reads its predecessor's toast.
 */
async function toastDrained(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toBe(null);
}

/** Stored layout key for one module as the server keeps it per user. */
async function moduleLayout(
  seed: ParitySeedFacts,
  projectId: string,
  moduleId: string,
  session: string
): Promise<string | null> {
  return (await serverModulesDetailUserProperties(seed.workspaceSlug, projectId, moduleId, session)).layout;
}

test.describe("module detail on desktop", () => {
  test(
    specTitle(["MOD-029"], "breadcrumb, switcher jump, badge, and tab title"),
    { tag: specTags(["MOD-029"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 frame ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectName = `${tag} project`;
      const projectId = await createModuleProject(seed, projectName, session);
      const nameA = `${tag} module A`;
      const nameB = `${tag} module B`;
      const idA = await serverCreateModule(seed.workspaceSlug, projectId, nameA, session);
      const idB = await serverCreateModule(seed.workspaceSlug, projectId, nameB, session);
      const issueIds: string[] = [];
      for (const name of [`${tag} issue A`, `${tag} issue B`]) {
        issueIds.push(await serverCreateIssue(seed.workspaceSlug, projectId, session, name));
      }
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, idA, issueIds, session);
      try {
        await openDetail(driver, seed, projectId, idA, nameA, session);

        await test.step("the breadcrumb names project, list, and module", async () => {
          const crumbs = await driver.modulesDetailCrumbs();
          expect(crumbs).toContain(projectName);
          expect(crumbs).toContain("Modules");
          expect(crumbs).toContain(nameA);
        });

        await test.step("the badge counts with an explanatory tooltip", async () => {
          await expect.poll(() => driver.modulesDetailBadgeText(), { timeout: 60_000 }).toBe("2");
          expect(await driver.modulesDetailBadgeTooltip()).toBe("There are 2 work items in this module");
          const detail = await serverModulesDetailGet(seed.workspaceSlug, projectId, idA, session);
          expect(detail.totalIssues).toBe(2);
        });

        await test.step("the tab title carries project and module", async () => {
          await expect
            .poll(() => driver.modulesDetailPageTitle(), { timeout: 60_000 })
            .toBe(`${projectName} - ${nameA}`);
        });

        await test.step("the searchable switcher jumps without leaving", async () => {
          await driver.modulesDetailSwitcherOpen();
          const options = await driver.modulesDetailSwitcherOptions();
          expect(options).toContain(nameA);
          expect(options).toContain(nameB);
          await driver.modulesDetailSwitcherSearch(nameB);
          await expect.poll(() => driver.modulesDetailSwitcherOptions(), { timeout: 30_000 }).toEqual([nameB]);
          await driver.modulesDetailSwitcherPick(nameB);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/modules/${idB}`);
          expect(await driver.modulesDetailSidebarName()).toBe(nameB);
        });

        await test.step("an empty module shows no badge", async () => {
          // The badge clears once module B's issues load; the jump lands
          // first, so settle on the empty state rather than racing it.
          await expect.poll(() => driver.modulesDetailBadgeText(), { timeout: 30_000 }).toBe(null);
          const detail = await serverModulesDetailGet(seed.workspaceSlug, projectId, idB, session);
          expect(detail.totalIssues).toBe(0);
        });

        await test.step("each crumb navigates to its level", async () => {
          await driver.modulesDetailClickModulesCrumb();
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/modules`);
          await openDetail(driver, seed, projectId, idB, nameB, session);
          await driver.modulesDetailClickProjectCrumb(projectName);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/issues`);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-029"], "unknown and deleted modules show the error empty state"),
    { tag: specTags(["MOD-029"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 gone ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const doomedId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} doomed`, session);
      await serverModulesDetailPatch(seed.workspaceSlug, projectId, doomedId, { name: `${tag} doomed` }, session);
      try {
        await test.step("an unknown id shows the error with a way back", async () => {
          await driver.openAuthenticated(
            `/${seed.workspaceSlug}/projects/${projectId}/modules/00000000-0000-0000-0000-000000000000`,
            sessionBrowserCookies(session)
          );
          await expect
            .poll(() => driver.modulesDetailUnknownTitle(), { timeout: 120_000 })
            .toBe("Module does not exist");
          await driver.modulesDetailUnknownBack();
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/modules`);
        });

        await test.step("a deleted module shows the same state", async () => {
          const path = `/${seed.workspaceSlug}/projects/${projectId}/modules/${doomedId}`;
          await driver.openAuthenticated(path, sessionBrowserCookies(session));
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(`${tag} doomed`);
          const res = await serverRequestStatus(
            "DELETE",
            `${parityApiBase()}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/modules/${doomedId}/`,
            session
          );
          expect(res.status).toBe(204);
          await driver.openAuthenticated(path, sessionBrowserCookies(session));
          await expect
            .poll(() => driver.modulesDetailUnknownTitle(), { timeout: 120_000 })
            .toBe("Module does not exist");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-030"], "detail layouts switch with display following the active layout"),
    { tag: specTags(["MOD-030"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 layouts ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("the header offers all five layouts", async () => {
          expect(await driver.layoutsOfferedLayouts()).toEqual([
            "list",
            "kanban",
            "calendar",
            "spreadsheet",
            "gantt_chart",
          ]);
        });

        await test.step("the filter toggle renders", async () => {
          expect(await driver.modulesDetailFilterToggleVisible()).toBe(true);
        });

        // Host-level proof only (layout internals belong to the Issues
        // area): the header marker follows each switch and the server
        // stores it per module.
        await test.step("board activates and the server stores the switch", async () => {
          await driver.modulesDetailSwitchLayout("kanban");
          expect(await driver.layoutsActiveLayout()).toBe("kanban");
          await expect.poll(() => moduleLayout(seed, projectId, moduleId, session), { timeout: 30_000 }).toBe("kanban");
        });

        await test.step("calendar activates and renders", async () => {
          await driver.modulesDetailSwitchLayout("calendar");
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await expect.poll(() => driver.layoutsCalendarVisible(), { timeout: 60_000 }).toBe(true);
        });

        await test.step("table activates", async () => {
          await driver.modulesDetailSwitchLayout("spreadsheet");
          expect(await driver.layoutsActiveLayout()).toBe("spreadsheet");
        });

        await test.step("timeline activates and renders", async () => {
          await driver.modulesDetailSwitchLayout("gantt_chart");
          expect(await driver.layoutsActiveLayout()).toBe("gantt_chart");
          await expect.poll(() => driver.layoutsGanttVisible(), { timeout: 120_000 }).toBe(true);
        });

        await test.step("display options follow the active layout", async () => {
          await driver.modulesDetailSwitchLayout("list");
          await driver.openDisplayOptions();
          const listPanel = await driver.displayPanelText();
          await driver.closeDisplayOptions();
          await driver.modulesDetailSwitchLayout("kanban");
          await driver.openDisplayOptions();
          const boardPanel = await driver.displayPanelText();
          await driver.closeDisplayOptions();
          expect(listPanel).not.toBe(boardPanel);
          expect(listPanel).toContain("Group by");
          expect(boardPanel).toContain("Group by");
        });

        await test.step("list activates again with the issue row", async () => {
          await driver.modulesDetailSwitchLayout("list");
          expect(await driver.layoutsActiveLayout()).toBe("list");
          await expect.poll(() => driver.layoutsListGroups(), { timeout: 60_000 }).toContain("Backlog");
          expect(await driver.layoutsListGroupIssueNames("Backlog")).toEqual([`${tag} issue`]);
          await expect.poll(() => moduleLayout(seed, projectId, moduleId, session), { timeout: 30_000 }).toBe("list");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-030"], "flag-off projects hide the cycle and module grouping entries"),
    { tag: specTags(["MOD-030"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 flagoff ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const offProjectId = await createModuleProject(seed, `${tag} off`, session, {});
      const offModuleId = await serverCreateModule(seed.workspaceSlug, offProjectId, `${tag} module`, session);
      const onProjectId = await createModuleProject(seed, `${tag} on`, session);
      const onModuleId = await serverCreateModule(seed.workspaceSlug, onProjectId, `${tag} module`, session);
      try {
        await test.step("the flag-off page hides cycle and module grouping", async () => {
          await openDetail(driver, seed, offProjectId, offModuleId, `${tag} module`, session);
          await driver.openDisplayOptions();
          const options = await driver.modulesDetailDisplayGroupedOptions();
          await driver.closeDisplayOptions();
          const labels = options.map((option) => option.label);
          expect(labels.length).toBeGreaterThan(0);
          expect(labels).not.toContain("Cycle");
          expect(labels).not.toContain("Module");
        });

        await test.step("the flag-on page offers cycle grouping but never module grouping", async () => {
          // The detail header always ignores module grouping (grouping a
          // module's own items by module is meaningless), so the Module
          // entry stays absent with the flag on too; Cycle follows the
          // project's cycle view flag.
          await openDetail(driver, seed, onProjectId, onModuleId, `${tag} module`, session);
          await driver.openDisplayOptions();
          const options = await driver.modulesDetailDisplayGroupedOptions();
          await driver.closeDisplayOptions();
          const labels = options.map((option) => option.label);
          expect(labels).toContain("Cycle");
          expect(labels).not.toContain("Module");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, offProjectId, session);
        await serverCleanupProject(seed.workspaceSlug, onProjectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-031"], "header add creates scoped to the module; empty state adds existing"),
    { tag: specTags(["MOD-031"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 add ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const plainName = `${tag} plain`;
      const plainId = await serverCreateIssue(seed.workspaceSlug, projectId, session, plainName);
      const otherName = `${tag} other`;
      await serverCreateIssue(seed.workspaceSlug, projectId, session, otherName);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("the header button opens module-scoped creation", async () => {
          expect(await driver.modulesDetailHeaderAddVisible()).toBe(true);
          await driver.modulesDetailHeaderAddClick();
          await expect.poll(() => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toBe(true);
          const createdName = `${tag} created`;
          await driver.layoutsWorkItemModalSetTitle(createdName);
          await driver.layoutsWorkItemModalSubmit();
          await expect
            .poll(() => serverModuleIssueIds(seed.workspaceSlug, projectId, moduleId, session), {
              timeout: 120_000,
            })
            .toHaveLength(1);
          await expect.poll(() => driver.modulesDetailBadgeText(), { timeout: 60_000 }).toBe("1");
        });

        // A second, empty module carries the empty-state branches.
        const freshId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} fresh`, session);

        await test.step("the empty module offers creation plus adding existing", async () => {
          await openDetail(driver, seed, projectId, freshId, `${tag} fresh`, session);
          await expect
            .poll(() => driver.layoutsEmptyTitle(), { timeout: 120_000 })
            .toBe("No work items to show in this Module");
          expect(await driver.layoutsEmptyActions()).toEqual([
            { label: "Create work item", disabled: false },
            { label: "Add existing work item", disabled: false },
          ]);
          await driver.layoutsEmptyChoose("Create work item");
          await expect.poll(() => driver.layoutsWorkItemModalVisible(), { timeout: 60_000 }).toBe(true);
          await driver.layoutsWorkItemModalClose();
        });

        await test.step("the searchable picker adds an existing item with a success notice", async () => {
          await driver.layoutsEmptyChoose("Add existing work item");
          await expect.poll(() => driver.layoutsAddExistingModalVisible(), { timeout: 60_000 }).toBe(true);
          await driver.modulesDetailPickerSearch(plainName);
          await expect.poll(() => driver.layoutsAddExistingModalIssueNames(), { timeout: 30_000 }).toEqual([plainName]);
          await driver.modulesDetailPickerSearch("");
          await driver.layoutsAddExistingModalChoose(plainName);
          expect(await toastText(driver)).toContain("Work items added to the module successfully.");
          await expect
            .poll(() => serverModuleIssueIds(seed.workspaceSlug, projectId, freshId, session), {
              timeout: 60_000,
            })
            .toContain(plainId);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-031"], "picker failures notice; filtered-empty offers clearing filters"),
    { tag: specTags(["MOD-031"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 addfail ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const plainName = `${tag} plain`;
      await serverCreateIssue(seed.workspaceSlug, projectId, session, plainName);
      const filterStateName = `${tag} filter state`;
      await serverCreateState(seed.workspaceSlug, projectId, filterStateName, "unstarted", session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("a refused add shows the failure notice and stores nothing", async () => {
          await driver.modulesDetailFailNextIssuesAdd();
          await driver.layoutsEmptyChoose("Add existing work item");
          await expect.poll(() => driver.layoutsAddExistingModalVisible(), { timeout: 60_000 }).toBe(true);
          await driver.layoutsAddExistingModalChoose(plainName);
          expect(await toastText(driver)).toContain("Selected work items could not be added to the module.");
          expect(await serverModuleIssueIds(seed.workspaceSlug, projectId, moduleId, session)).toEqual([]);
          await driver.openAuthenticated(
            `/${seed.workspaceSlug}/projects/${projectId}/modules/${moduleId}`,
            sessionBrowserCookies(session)
          );
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(moduleName);
        });

        await test.step("filtered-but-empty offers clearing its filters instead", async () => {
          // The seeded condition empties the rows but leaves the fresh
          // card; a second condition added through the UI completes the
          // filtered-empty branch (ISS-070 precedent).
          await serverPatchModuleUserProperties(seed.workspaceSlug, projectId, moduleId, session, {
            rich_filters: MATCH_NOTHING,
          });
          await driver.openAuthenticated(
            `/${seed.workspaceSlug}/projects/${projectId}/modules/${moduleId}`,
            sessionBrowserCookies(session)
          );
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(moduleName);
          await driver.layoutsFilterAddConditionViaRow("State", filterStateName);
          await expect.poll(() => driver.layoutsEmptyTitle(), { timeout: 120_000 }).toBe("No matching results.");
          expect(await driver.layoutsEmptyActions()).toEqual([{ label: "Clear filters", disabled: false }]);
          await driver.layoutsEmptyChoose("Clear filters");
          await expect
            .poll(() => driver.layoutsEmptyTitle(), { timeout: 120_000 })
            .toBe("No work items to show in this Module");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(
      ["MOD-029", "MOD-031", "MOD-033"],
      "guests read the detail page with every control disabled (NEWFRONT-334)"
    ),
    { tag: specTags(["MOD-029", "MOD-031", "MOD-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 guest ${Date.now()}`;
      const guest = requireGuest(seed);
      const session = await signInSession(seed.email, seed.password);
      const guestSession = await signInSession(guest.email, guest.password);
      const projectName = `${tag} project`;
      const projectId = await createModuleProject(seed, projectName, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const otherName = `${tag} other`;
      const otherId = await serverCreateModule(seed.workspaceSlug, projectId, otherName, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [issueId], session);
      await serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session);
      try {
        await test.step("the server answers the guest's detail and item reads", async () => {
          const base = parityApiBase();
          const detail = await serverRequestStatus(
            "GET",
            `${base}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/modules/${moduleId}/`,
            guestSession
          );
          expect(detail.status).toBe(200);
          const issues = await serverRequestStatus(
            "GET",
            `${base}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/modules/${moduleId}/issues/`,
            guestSession
          );
          expect(issues.status).toBe(200);
          const list = await serverRequestStatus(
            "GET",
            `${base}/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/modules/`,
            guestSession
          );
          expect(list.status).toBe(200);
          // The fixture issue belongs to a member, so the default guest
          // flag scopes the guest's item read to nothing (NEWFRONT-334).
          expect(await serverModuleIssueIds(seed.workspaceSlug, projectId, moduleId, guestSession)).toEqual([]);
          expect(await serverModuleIssueIds(seed.workspaceSlug, projectId, moduleId, session)).toEqual([issueId]);
        });

        await test.step("the guest sees the frame without the error state", async () => {
          await openDetail(driver, seed, projectId, moduleId, moduleName, guestSession);
          expect(await driver.modulesDetailUnknownTitle()).toBe(null);
          const crumbs = await driver.modulesDetailCrumbs();
          expect(crumbs).toContain(projectName);
          expect(crumbs).toContain("Modules");
          expect(crumbs).toContain(moduleName);
          await expect
            .poll(() => driver.modulesDetailPageTitle(), { timeout: 60_000 })
            .toBe(`${projectName} - ${moduleName}`);
          // The badge counts loaded items, so the scoped-empty list hides
          // it; settle on the empty state first so the read never races
          // the fetch.
          await expect
            .poll(() => driver.layoutsEmptyTitle(), { timeout: 120_000 })
            .toBe("No work items to show in this Module");
          await expect.poll(() => driver.modulesDetailBadgeText(), { timeout: 30_000 }).toBe(null);
        });

        await test.step("the empty list offers only disabled actions", async () => {
          expect(await driver.layoutsEmptyActions()).toEqual([
            { label: "Create work item", disabled: true },
            { label: "Add existing work item", disabled: true },
          ]);
          expect(await driver.modulesDetailHeaderAddVisible()).toBe(false);
        });

        await test.step("the sidebar reads but cannot edit", async () => {
          expect(await driver.modulesDetailSidebarName()).toBe(moduleName);
          expect(await driver.modulesDetailSidebarStatus()).not.toBe(null);
          expect(await driver.modulesDetailSidebarStatusDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarDateDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarLeadDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarMembersDisabled()).toBe(true);
        });

        await test.step("the switcher jumps without leaving", async () => {
          await driver.modulesDetailSwitcherOpen();
          const options = await driver.modulesDetailSwitcherOptions();
          expect(options).toContain(moduleName);
          expect(options).toContain(otherName);
          await driver.modulesDetailSwitcherSearch(otherName);
          await expect.poll(() => driver.modulesDetailSwitcherOptions(), { timeout: 30_000 }).toEqual([otherName]);
          await driver.modulesDetailSwitcherPick(otherName);
          await expect
            .poll(() => currentPathClean(driver), { timeout: 30_000 })
            .toBe(`/${seed.workspaceSlug}/projects/${projectId}/modules/${otherId}`);
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(otherName);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-032"], "analytics opens for the module; sidebar collapse persists"),
    { tag: specTags(["MOD-032"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 analytics ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectName = `${tag} project`;
      const projectId = await createModuleProject(seed, projectName, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("analytics opens the module's dialog", async () => {
          expect(await driver.modulesDetailAnalyticsVisible()).toBe(true);
          await driver.modulesDetailOpenAnalytics();
          expect(await driver.modulesDetailAnalyticsDialogVisible()).toBe(true);
          // Dialog content belongs to the analytics area; the module
          // surface proves it opens scoped to this module.
          expect(await driver.analyticsDialogText()).toContain(moduleName);
          await driver.modulesDetailDismissAnalytics();
          expect(await driver.modulesDetailAnalyticsDialogVisible()).toBe(false);
        });

        await test.step("the header toggle hides the sidebar", async () => {
          expect(await driver.modulesDetailSidebarCollapsedStored()).toBe(false);
          await driver.modulesDetailToggleSidebar();
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 30_000 }).toBe(null);
          expect(await driver.modulesDetailSidebarVisible()).toBe(false);
          expect(await driver.modulesDetailSidebarCollapsedStored()).toBe(true);
        });

        await test.step("a reload keeps it hidden", async () => {
          await driver.layoutsReloadIssues();
          await expect.poll(() => driver.modulesDetailSidebarCollapsedStored(), { timeout: 30_000 }).toBe(true);
          expect(await driver.modulesDetailSidebarName()).toBe(null);
        });

        await test.step("the header toggle shows it again", async () => {
          await driver.modulesDetailToggleSidebar();
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 30_000 }).toBe(moduleName);
          expect(await driver.modulesDetailSidebarVisible()).toBe(true);
          expect(await driver.modulesDetailSidebarCollapsedStored()).toBe(false);
        });

        await test.step("the sidebar's own close control hides it too", async () => {
          await driver.modulesDetailCloseSidebarViaPanel();
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 30_000 }).toBe(null);
          expect(await driver.modulesDetailSidebarCollapsedStored()).toBe(true);
          await driver.modulesDetailToggleSidebar();
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 30_000 }).toBe(moduleName);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-033"], "sidebar shows identity, dates, lead, members, and counts"),
    { tag: specTags(["MOD-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 fields ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const owner = await serverMe(session);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const description = `${tag} description`;
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        moduleId,
        {
          description,
          status: "in-progress",
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
          lead_id: owner.id,
          member_ids: [owner.id],
        },
        session
      );
      const backlogId = await createStateIssue(seed, projectId, `${tag} backlog`, "backlog", session);
      const doneId = await createStateIssue(seed, projectId, `${tag} done`, "done", session);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [backlogId, doneId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("identity, dates, lead, and members render", async () => {
          expect(await driver.modulesDetailSidebarStatus()).toBe("In Progress");
          expect(await driver.modulesDetailSidebarDescription()).toBe(description);
          expect(await driver.modulesDetailSidebarDateText()).toContain("Sep 01");
          // Live the rows show avatar initials, never display names: the
          // lead row carries the lead's initial plus the unresolved-name
          // placeholder; identity is proven by server below.
          if (driver.target === "web_new") {
            // The new picker resolves display names where the old row
            // kept the placeholder.
            expect(await driver.modulesDetailSidebarLead()).toBe(`${owner.displayName.charAt(0)} ${owner.displayName}`);
            // Member avatars resolve after the module record settles.
            await expect
              .poll(() => driver.modulesDetailSidebarMembers(), { timeout: 60_000 })
              .toEqual([owner.displayName]);
          } else {
            expect(await driver.modulesDetailSidebarLead()).toBe(`${owner.displayName.charAt(0)} Lead`);
            // Member avatars resolve after the module record settles.
            await expect
              .poll(() => driver.modulesDetailSidebarMembers(), { timeout: 60_000 })
              .toEqual([owner.displayName.charAt(0)]);
          }
        });

        await test.step("counts render without a points row", async () => {
          await expect.poll(() => driver.modulesDetailSidebarWorkItemsText(), { timeout: 60_000 }).toBe("1/2");
          expect(await driver.modulesDetailSidebarPointsText()).toBe(null);
        });

        await test.step("the server record matches", async () => {
          const detail = await serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session);
          expect(detail.status).toBe("in-progress");
          expect(detail.description).toBe(description);
          expect(detail.startDate?.slice(0, 10)).toBe(ACTIVE_START);
          expect(detail.targetDate?.slice(0, 10)).toBe(ACTIVE_END);
          expect(detail.leadId).toBe(owner.id);
          expect(detail.memberIds).toContain(owner.id);
          expect(detail.totalIssues).toBe(2);
          expect(detail.completedIssues).toBe(1);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-033"], "inline edits save with confirmation"),
    { tag: specTags(["MOD-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 edits ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const teammate = requireMention(seed);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      await serverAddProjectMembers(
        seed.workspaceSlug,
        projectId,
        [{ memberId: teammate.id, role: ROLE_MEMBER }],
        session
      );
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        moduleId,
        {
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
        },
        session
      );
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("status saves inline", async () => {
          expect(await driver.modulesDetailSidebarStatusDisabled()).toBe(false);
          await driver.modulesDetailSidebarSetStatus("In Progress");
          await expect
            .poll(async () => (await serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session)).status, {
              timeout: 30_000,
            })
            .toBe("in-progress");
        });

        await test.step("moving the end saves with a success confirmation", async () => {
          expect(await driver.modulesDetailSidebarDateDisabled()).toBe(false);
          await driver.modulesDetailSidebarOpenDatePicker();
          await driver.modulesDetailPickDateDay("October 15th, 2026");
          await expect
            .poll(
              async () => (await serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session)).targetDate,
              { timeout: 30_000 }
            )
            .toContain("2026-10-15");
          await driver.modulesDetailPickDateDay("October 18th, 2026");
          await expect
            .poll(async () => serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session), {
              timeout: 30_000,
            })
            .toMatchObject({ targetDate: expect.stringContaining("2026-10-18") });
          await driver.modulesDetailDismissDatePicker();
          expect(await toastText(driver)).toContain("Module updated successfully.");
          expect(await driver.modulesDetailSidebarDateText()).toBe("Sep 01, 2026 Oct 18, 2026");
        });

        await test.step("lead saves inline", async () => {
          // The member options name the viewing owner "You" and hide
          // guests, so the second member (addressed by display name)
          // carries the pick proof.
          expect(await driver.modulesDetailSidebarLeadDisabled()).toBe(false);
          expect(await driver.modulesDetailSidebarLead()).toBe("Lead");
          await driver.modulesDetailSidebarSetLead(teammate.displayName);
          await expect
            .poll(async () => (await serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session)).leadId, {
              timeout: 30_000,
            })
            .toBe(teammate.id);
        });

        await test.step("members save inline", async () => {
          expect(await driver.modulesDetailSidebarMembersDisabled()).toBe(false);
          expect(await driver.modulesDetailSidebarMembers()).toEqual([]);
          await driver.modulesDetailSidebarAddMember(teammate.displayName);
          await expect
            .poll(
              async () => (await serverModulesDetailGet(seed.workspaceSlug, projectId, moduleId, session)).memberIds,
              { timeout: 30_000 }
            )
            .toContain(teammate.id);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-033"], "points row renders only under points estimates"),
    { tag: specTags(["MOD-033"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 points ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const pointsProjectId = await createModuleProject(seed, `${tag} points project`, session);
      const estimate = await serverCreateEstimate(
        seed.workspaceSlug,
        pointsProjectId,
        `${tag} est`,
        ["1", "2", "3"],
        session
      );
      await serverSetProjectEstimate(seed.workspaceSlug, pointsProjectId, estimate.id, session);
      const timeProjectId = await createModuleProject(seed, `${tag} time project`, session);
      const timeEstimate = await serverCreateEstimate(
        seed.workspaceSlug,
        timeProjectId,
        `${tag} time est`,
        ["1", "2"],
        session,
        "time"
      );
      await serverSetProjectEstimate(seed.workspaceSlug, timeProjectId, timeEstimate.id, session);
      const pointsModuleId = await serverCreateModule(seed.workspaceSlug, pointsProjectId, `${tag} module`, session);
      const timeModuleId = await serverCreateModule(seed.workspaceSlug, timeProjectId, `${tag} module`, session);
      const pointsIssueId = await createStateIssue(seed, pointsProjectId, `${tag} issue`, "backlog", session);
      await serverPatchIssue(
        seed.workspaceSlug,
        pointsProjectId,
        pointsIssueId,
        { estimate_point: estimate.points[1]?.id },
        session
      );
      await serverAddIssuesToModule(seed.workspaceSlug, pointsProjectId, pointsModuleId, [pointsIssueId], session);
      const timeIssueId = await serverCreateIssue(seed.workspaceSlug, timeProjectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, timeProjectId, timeModuleId, [timeIssueId], session);
      try {
        await test.step("the points project shows completed-over-total points", async () => {
          await openDetail(driver, seed, pointsProjectId, pointsModuleId, `${tag} module`, session);
          expect(await driver.modulesDetailSidebarWorkItemsText()).toBe("0/1");
          await expect.poll(() => driver.modulesDetailSidebarPointsText(), { timeout: 60_000 }).toBe("0/2");
          const detail = await serverModulesDetailGet(seed.workspaceSlug, pointsProjectId, pointsModuleId, session);
          expect(detail.totalPoints).toBe(2);
          expect(detail.completedPoints).toBe(0);
        });

        await test.step("the time-estimate project shows no points row", async () => {
          await openDetail(driver, seed, timeProjectId, timeModuleId, `${tag} module`, session);
          expect(await driver.modulesDetailSidebarWorkItemsText()).toBe("0/1");
          expect(await driver.modulesDetailSidebarPointsText()).toBe(null);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, pointsProjectId, session);
        await serverCleanupProject(seed.workspaceSlug, timeProjectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-033"], "fields disable on archived modules; loader shows while fetching"),
    { tag: specTags(["MOD-033"]) },
    async ({ driver, seed }) => {
      // The guest scenario above pins guest-disabled fields on the live
      // detail page (NEWFRONT-334); the archived branch below carries the
      // archived-disabled proof.
      const tag = `NF310 disabled ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        moduleId,
        {
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
        },
        session
      );
      const archivedId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} archived`, session);
      // Archiving refuses modules that are not completed or cancelled.
      await serverModulesDetailPatch(seed.workspaceSlug, projectId, archivedId, { status: "completed" }, session);
      await archivesArchiveModule(seed.workspaceSlug, projectId, archivedId, session);
      try {
        await test.step("archived modules read but cannot edit", async () => {
          // The detail route shows the missing-module state for archived
          // modules, so the archived peek (which mounts the same sidebar
          // with edits refused) carries the disabled proof.
          await driver.openAuthenticated(
            `/${seed.workspaceSlug}/projects/${projectId}/modules`,
            sessionBrowserCookies(session)
          );
          await driver.archivesOpenModulesTab(seed.workspaceSlug, projectId);
          await driver.archivesAwaitModuleRow(`${tag} archived`);
          await driver.archivesOpenModulePeek(`${tag} archived`);
          expect(await driver.modulesDetailSidebarName()).toBe(`${tag} archived`);
          expect(await driver.modulesDetailSidebarStatusDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarDateDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarLeadDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarMembersDisabled()).toBe(true);
          expect(await driver.modulesDetailSidebarStatus()).not.toBe(null);
          await driver.archivesCloseModulePeek();
        });

        await test.step("a loader shows while details fetch", async () => {
          await openDetail(driver, seed, projectId, moduleId, moduleName, session);
          expect(await driver.modulesDetailSlowFetchShowsLoader()).toBe(true);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-034"], "active modules chart with a plot switch that refetches"),
    { tag: specTags(["MOD-034"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 chart ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const estimate = await serverCreateEstimate(
        seed.workspaceSlug,
        projectId,
        `${tag} est`,
        ["1", "2", "3"],
        session
      );
      await serverSetProjectEstimate(seed.workspaceSlug, projectId, estimate.id, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        moduleId,
        {
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
        },
        session
      );
      const backlogId = await createStateIssue(seed, projectId, `${tag} backlog`, "backlog", session);
      await serverPatchIssue(
        seed.workspaceSlug,
        projectId,
        backlogId,
        { estimate_point: estimate.points[1]?.id },
        session
      );
      const doneId = await createStateIssue(seed, projectId, `${tag} done`, "done", session);
      await serverPatchIssue(
        seed.workspaceSlug,
        projectId,
        doneId,
        { estimate_point: estimate.points[0]?.id },
        session
      );
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [backlogId, doneId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("the section opens with percent, chart, and measure", async () => {
          // The disclosure body mounts after the completion data lands,
          // later than the sidebar name openDetail settles on.
          await expect.poll(() => driver.modulesDetailProgressOpen(), { timeout: 60_000 }).toBe(true);
          expect(await driver.modulesDetailProgressNotice()).toBe(null);
          await expect.poll(() => driver.modulesDetailProgressHeaderPercent(), { timeout: 60_000 }).toBe(50);
          expect(await driver.modulesDetailProgressChartVisible()).toBe(true);
          expect(await driver.modulesDetailProgressMeasureValue()).toBe("Work items");
          expect(await driver.modulesDetailProgressMeasureOptions()).toEqual(["Work items", "Points"]);
        });

        await test.step("switching measure refetches the details", async () => {
          // Breakdown row content belongs to the bug: scenario below
          // (NEWFRONT-337 rotates tab contents); this step proves the
          // switch itself refetches and keeps the chart.
          const gets = await driver.modulesDetailMeasureSwitchGetCount("Points");
          expect(gets).toBeGreaterThanOrEqual(1);
          expect(await driver.modulesDetailProgressMeasureValue()).toBe("Points");
          expect(await driver.modulesDetailProgressChartVisible()).toBe(true);
          const back = await driver.modulesDetailMeasureSwitchGetCount("Work items");
          expect(back).toBeGreaterThanOrEqual(1);
        });

        await test.step("no measure dropdown without points estimates", async () => {
          const plainProjectId = await createModuleProject(seed, `${tag} plain project`, session);
          const plainModuleId = await serverCreateModule(seed.workspaceSlug, plainProjectId, `${tag} plain`, session);
          await serverModulesDetailPatch(
            seed.workspaceSlug,
            plainProjectId,
            plainModuleId,
            {
              start_date: ACTIVE_START,
              target_date: ACTIVE_END,
            },
            session
          );
          try {
            await openDetail(driver, seed, plainProjectId, plainModuleId, `${tag} plain`, session);
            // The disclosure body mounts after the completion data lands,
            // later than the sidebar name openDetail settles on.
            await expect.poll(() => driver.modulesDetailProgressOpen(), { timeout: 60_000 }).toBe(true);
            expect(await driver.modulesDetailProgressMeasureValue()).toBe(null);
            expect(await driver.modulesDetailProgressMeasureOptions()).toEqual([]);
          } finally {
            await serverCleanupProject(seed.workspaceSlug, plainProjectId, session);
          }
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-034"], "not-started and invalid dates explain instead of charting"),
    { tag: specTags(["MOD-034"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 dates ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const futureId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} future`, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        futureId,
        {
          start_date: FUTURE_START,
          target_date: FUTURE_END,
        },
        session
      );
      const missingId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} missing`, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        missingId,
        {
          start_date: null,
          target_date: null,
        },
        session
      );
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, futureId, [issueId], session);
      try {
        await test.step("a future start explains the module is not active yet", async () => {
          await openDetail(driver, seed, projectId, futureId, `${tag} future`, session);
          expect(await driver.modulesDetailProgressOpen()).toBe(false);
          expect(await driver.modulesDetailProgressNotice()).toBe("This module isn't active yet.");
          expect(await driver.modulesDetailProgressChartVisible()).toBe(false);
          expect(await driver.modulesDetailProgressMeasureValue()).toBe(null);
        });

        await test.step("missing dates explain the dates are invalid", async () => {
          await openDetail(driver, seed, projectId, missingId, `${tag} missing`, session);
          expect(await driver.modulesDetailProgressOpen()).toBe(false);
          expect(await driver.modulesDetailProgressNotice()).toBe("Invalid date. Please enter valid date.");
          expect(await driver.modulesDetailProgressChartVisible()).toBe(false);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(
      ["MOD-034"],
      "bug: NEWFRONT-337 breakdown tabs remember per module but show rotated contents; entries filter the list; read-only in peek"
    ),
    { tag: specTags(["MOD-034"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 breakdown ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const nameA = `${tag} module A`;
      const idA = await serverCreateModule(seed.workspaceSlug, projectId, nameA, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        idA,
        {
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
        },
        session
      );
      const nameB = `${tag} module B`;
      const idB = await serverCreateModule(seed.workspaceSlug, projectId, nameB, session);
      await serverModulesDetailPatch(
        seed.workspaceSlug,
        projectId,
        idB,
        {
          start_date: ACTIVE_START,
          target_date: ACTIVE_END,
        },
        session
      );
      const backlogName = `${tag} backlog`;
      const todoName = `${tag} todo`;
      const doneName = `${tag} done`;
      const backlog = await createStateIssue(seed, projectId, backlogName, "backlog", session);
      const todo = await createStateIssue(seed, projectId, todoName, "todo", session);
      const done = await createStateIssue(seed, projectId, doneName, "done", session);
      const labelName = `${tag} label`;
      const label = await serverCreateLabel(seed.workspaceSlug, projectId, labelName, "#ff0000", session);
      await serverPatchIssue(seed.workspaceSlug, projectId, backlog, { label_ids: [label.id] }, session);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, idA, [backlog, todo, done], session);
      const teammate = requireMention(seed);
      await serverAddProjectMembers(
        seed.workspaceSlug,
        projectId,
        [{ memberId: teammate.id, role: ROLE_MEMBER }],
        session
      );
      await serverPatchIssue(seed.workspaceSlug, projectId, todo, { assignee_ids: [teammate.id] }, session);
      const peekIssue = await createStateIssue(seed, projectId, `${tag} peek issue`, "backlog", session);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, idB, [peekIssue], session);
      try {
        await openDetail(driver, seed, projectId, idA, nameA, session);
        await driver.modulesDetailSwitchLayout("list");

        await test.step("each module remembers its own breakdown tab", async () => {
          expect(await driver.modulesDetailActiveStatsTab()).toBe("Assignees");
          await driver.modulesDetailProgressStatsTab("Labels");
          expect(await driver.modulesDetailActiveStatsTab()).toBe("Labels");
          await openDetail(driver, seed, projectId, idB, nameB, session);
          expect(await driver.modulesDetailActiveStatsTab()).toBe("Assignees");
          await openDetail(driver, seed, projectId, idA, nameA, session);
          expect(await driver.modulesDetailActiveStatsTab()).toBe("Labels");
        });

        await test.step("live each tab shows the next dimension's breakdown (NEWFRONT-337)", async () => {
          await driver.modulesDetailProgressStatsTab("States");
          let rows = await driver.modulesDetailProgressStatsRows();
          expect(rows.map((row) => row.title)).toContain("No assignee");
          await driver.modulesDetailProgressStatsTab("Assignees");
          rows = await driver.modulesDetailProgressStatsRows();
          expect(rows.map((row) => row.title)).toContain(labelName);
          await driver.modulesDetailProgressStatsTab("Labels");
          rows = await driver.modulesDetailProgressStatsRows();
          expect(rows.find((row) => row.title === "Backlog")).toEqual({ title: "Backlog", percent: 33, total: 3 });
        });

        await test.step("entries filter the list on detail", async () => {
          // Entries wire their handlers once the filter instance loads;
          // settle on the clickable marker instead of racing that window.
          await driver.modulesDetailProgressStatsTab("Labels");
          await expect
            .poll(() => driver.modulesDetailProgressEntryClickable("Backlog"), { timeout: 30_000 })
            .toBe(true);
          const listNames = () => driver.layoutsListGroupIssueNames("All work items");
          const richFiltersText = async () =>
            JSON.stringify(
              (await serverModulesDetailUserProperties(seed.workspaceSlug, projectId, idA, session)).richFilters
            );
          const allNames = expect.arrayContaining([backlogName, todoName, doneName]);
          // A breakdown click that lands while the panel re-renders can
          // miss (swallowed, no error): re-click until the entry's marker
          // flips, which proves the click registered, and only then read
          // the list. Retries never toggle blindly, so they converge.
          const clickUntilList = async (entry: string, expected: unknown) => {
            for (let i = 0; i < 3; i++) {
              const wasSelected = await driver.modulesDetailProgressEntrySelected(entry);
              await driver.modulesDetailProgressEntryClick(entry);
              try {
                await expect
                  .poll(() => driver.modulesDetailProgressEntrySelected(entry), { timeout: 10_000 })
                  .toBe(!wasSelected);
              } catch {
                // Swallowed; the marker is untouched, so click again.
                continue;
              }
              await expect.poll(listNames, { timeout: 30_000 }).toEqual(expected);
              return;
            }
            await expect.poll(listNames, { timeout: 30_000 }).toEqual(expected);
          };
          // State groups (live under the Labels tab while NEWFRONT-337 stands).
          await clickUntilList("Backlog", [backlogName]);
          await expect.poll(richFiltersText, { timeout: 30_000 }).toContain("backlog");
          await clickUntilList("Backlog", allNames);
          await expect.poll(richFiltersText, { timeout: 30_000 }).not.toContain("state_group");
          // Assignees (live under the States tab while NEWFRONT-337 stands).
          await driver.modulesDetailProgressStatsTab("States");
          await clickUntilList(teammate.displayName, [todoName]);
          await expect.poll(richFiltersText, { timeout: 30_000 }).toContain(teammate.id);
          await clickUntilList(teammate.displayName, allNames);
          await expect.poll(richFiltersText, { timeout: 30_000 }).not.toContain(teammate.id);
          // Labels (live under the Assignees tab while NEWFRONT-337 stands).
          await driver.modulesDetailProgressStatsTab("Assignees");
          await clickUntilList(labelName, [backlogName]);
          await expect.poll(richFiltersText, { timeout: 30_000 }).toContain(label.id);
          await clickUntilList(labelName, allNames);
          await expect.poll(richFiltersText, { timeout: 30_000 }).not.toContain(label.id);
        });

        await test.step("peek breakdowns switch tabs but write no filter", async () => {
          await driver.openAuthenticated(
            `/${seed.workspaceSlug}/projects/${projectId}/modules`,
            sessionBrowserCookies(session)
          );
          // The peek opener reads the row the moment it runs, so settle
          // the list and await the row first (archives spec pattern).
          await driver.archivesOpenLiveModules(seed.workspaceSlug, projectId);
          await driver.archivesAwaitModuleRow(nameB);
          await driver.archivesOpenLiveModulePeek(nameB);
          await driver.modulesDetailProgressStatsTab("Labels");
          expect(await driver.modulesDetailActiveStatsTab()).toBe("Labels");
          expect(await driver.modulesDetailProgressEntryClickable("Backlog")).toBe(false);
          await driver.modulesDetailProgressEntryClick("Backlog");
          await driver.page.waitForTimeout(3_000);
          const filters = JSON.stringify(
            (await serverModulesDetailUserProperties(seed.workspaceSlug, projectId, idB, session)).richFilters
          );
          expect(filters).not.toContain("backlog");
          await driver.archivesCloseModulePeek();
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-035"], "bug: NEWFRONT-342 links section states and dialog validation with scheme defaulting"),
    { tag: specTags(["MOD-035"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 links ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("the empty section stays collapsed with a notice and add action", async () => {
          expect(await driver.modulesDetailLinksOpen()).toBe(false);
          await driver.modulesDetailLinksToggle();
          expect(await driver.modulesDetailLinksOpen()).toBe(true);
          expect(await driver.modulesDetailLinksEmptyNotice()).toBe("No links added yet");
          expect(await driver.modulesDetailLinksAddVisible()).toBe(true);
          expect(await driver.modulesDetailLinksRows()).toEqual([]);
        });

        await test.step("the dialog requires a URL", async () => {
          await driver.modulesDetailLinksAddClick();
          expect(await driver.modulesDetailLinkDialogOpen()).toBe(true);
          expect(await driver.modulesDetailLinkDialogTitle()).toBe("Add link");
          await driver.modulesDetailLinkSubmit();
          // No error text renders; refusal is the open dialog, the
          // invalid-marked field, and nothing stored.
          expect(await driver.modulesDetailLinkUrlInvalid()).toBe(true);
          expect(await driver.modulesDetailLinkDialogOpen()).toBe(true);
          expect(await serverModulesDetailLinks(seed.workspaceSlug, projectId, moduleId, session)).toEqual([]);
          await driver.modulesDetailLinkCancel();
          expect(await driver.modulesDetailLinkDialogOpen()).toBe(false);
        });

        await test.step("a missing scheme is defaulted to http", async () => {
          await driver.modulesDetailLinksAddClick();
          await driver.modulesDetailLinkFillUrl("example.com");
          await driver.modulesDetailLinkSubmit();
          expect(await toastText(driver)).toContain("Module link created successfully.");
          await toastDrained(driver);
          await expect.poll(() => driver.modulesDetailLinkDialogOpen(), { timeout: 30_000 }).toBe(false);
          await expect
            .poll(() => driver.modulesDetailLinksRows(), { timeout: 30_000 })
            .toEqual([{ title: "http://example.com", url: "http://example.com", target: "_blank" }]);
          const links = await serverModulesDetailLinks(seed.workspaceSlug, projectId, moduleId, session);
          expect(links.map((link) => link.url)).toEqual(["http://example.com"]);
        });

        await test.step("bug: NEWFRONT-342 the section stays collapsed after reload though links exist", async () => {
          await driver.layoutsReloadIssues();
          await expect.poll(() => driver.modulesDetailSidebarName(), { timeout: 120_000 }).toBe(moduleName);
          if (driver.target === "web_new") {
            // Fixed (NEWFRONT-342): the section opens by default once
            // the stored link loads, so the rows render with no toggle.
            await expect.poll(() => driver.modulesDetailLinksOpen(), { timeout: 30_000 }).toBe(true);
            await expect
              .poll(() => driver.modulesDetailLinksRows(), { timeout: 30_000 })
              .toEqual([{ title: "http://example.com", url: "http://example.com", target: "_blank" }]);
            expect(await driver.modulesDetailLinksEmptyNotice()).toBe(null);
          } else {
            // Live the app stays collapsed, so settle, pin it closed,
            // then prove the rows are one toggle away (the data is
            // there — only the default-open is not).
            await driver.page.waitForTimeout(5_000);
            expect(await driver.modulesDetailLinksOpen()).toBe(false);
            await driver.modulesDetailLinksToggle();
            await expect
              .poll(() => driver.modulesDetailLinksRows(), { timeout: 30_000 })
              .toEqual([{ title: "http://example.com", url: "http://example.com", target: "_blank" }]);
            expect(await driver.modulesDetailLinksEmptyNotice()).toBe(null);
          }
        });

        await test.step("a refused create shows the failure notice and stores nothing", async () => {
          const secondId = await serverCreateModule(seed.workspaceSlug, projectId, `${tag} second`, session);
          await openDetail(driver, seed, projectId, secondId, `${tag} second`, session);
          await driver.modulesDetailLinksToggle();
          await driver.modulesDetailFailNextLinkWrite();
          await driver.modulesDetailLinksAddClick();
          await driver.modulesDetailLinkFillUrl("https://refused.example");
          await driver.modulesDetailLinkSubmit();
          expect(await toastText(driver)).toContain("Error!");
          expect(await serverModulesDetailLinks(seed.workspaceSlug, projectId, secondId, session)).toEqual([]);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-035"], "link rows open, copy, edit, and delete with notices and server state"),
    { tag: specTags(["MOD-035"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 linkrows ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const owner = await serverMe(session);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const titledName = `${tag} docs`;
      const titledUrl = "https://docs.example.com/guide";
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);
        await driver.modulesDetailLinksToggle();

        await test.step("titled and untitled links render with meta", async () => {
          await driver.modulesDetailLinksAddClick();
          await driver.modulesDetailLinkFillUrl(titledUrl);
          await driver.modulesDetailLinkFillTitle(titledName);
          await driver.modulesDetailLinkSubmit();
          expect(await toastText(driver)).toContain("Module link created successfully.");
          await toastDrained(driver);
          await driver.modulesDetailLinksAddClick();
          await driver.modulesDetailLinkFillUrl("https://plain.example.com");
          await driver.modulesDetailLinkSubmit();
          expect(await toastText(driver)).toContain("Module link created successfully.");
          await toastDrained(driver);
          // Both writes are toast-confirmed; settle the server first so a
          // slow list refresh never reads a half-converged client copy.
          await expect
            .poll(
              async () => (await serverModulesDetailLinks(seed.workspaceSlug, projectId, moduleId, session)).length,
              { timeout: 30_000 }
            )
            .toBe(2);
          await expect
            .poll(() => driver.modulesDetailLinksRows(), { timeout: 60_000 })
            .toEqual([
              { title: titledName, url: titledUrl, target: "_blank" },
              { title: "https://plain.example.com", url: "https://plain.example.com", target: "_blank" },
            ]);
          const meta = await driver.modulesDetailLinkMeta(titledName);
          expect(meta).toContain("Added");
          expect(meta).toContain(owner.displayName);
        });

        await test.step("copying confirms with a notice", async () => {
          await driver.modulesDetailLinkCopy(titledName);
          expect(await toastText(driver)).toContain("Copied to clipboard");
          await toastDrained(driver);
        });

        await test.step("editing updates with a notice and server state", async () => {
          await driver.modulesDetailLinkEdit(titledName);
          expect(await driver.modulesDetailLinkDialogTitle()).toBe("Update link");
          const renamed = `${titledName} v2`;
          await driver.modulesDetailLinkFillTitle(renamed);
          await driver.modulesDetailLinkSubmit();
          expect(await toastText(driver)).toContain("Module link updated successfully.");
          await toastDrained(driver);
          await expect
            .poll(() => driver.modulesDetailLinksRows(), { timeout: 30_000 })
            .toEqual(expect.arrayContaining([{ title: renamed, url: titledUrl, target: "_blank" }]));
          const links = await serverModulesDetailLinks(seed.workspaceSlug, projectId, moduleId, session);
          expect(links.map((link) => link.title)).toContain(renamed);
        });

        await test.step("deleting removes with a notice and server state", async () => {
          await driver.modulesDetailLinkDelete("https://plain.example.com");
          expect(await toastText(driver)).toContain("Module link deleted successfully.");
          await expect.poll(() => driver.modulesDetailLinksRows(), { timeout: 30_000 }).toHaveLength(1);
          const links = await serverModulesDetailLinks(seed.workspaceSlug, projectId, moduleId, session);
          expect(links).toHaveLength(1);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );
});

test.describe("module detail on small screens", () => {
  test.use({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
  });

  test(
    specTitle(["MOD-030"], "small screens offer list, board, and calendar with display"),
    { tag: specTags(["MOD-030"]) },
    async ({ driver, seed }) => {
      const tag = `NF310 mobilelayout ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("the layout menu holds three layouts", async () => {
          expect(await driver.modulesDetailMobileOfferedLayouts()).toEqual(["list", "kanban", "calendar"]);
        });

        await test.step("the bar offers display but no filter toggle", async () => {
          expect(await driver.modulesDetailMobileDisplayVisible()).toBe(true);
          expect(await driver.modulesDetailMobileFilterToggleVisible()).toBe(false);
        });

        await test.step("switching persists across a reload", async () => {
          await driver.modulesDetailMobileSwitchTo("calendar");
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await driver.layoutsReloadIssues();
          expect(await driver.layoutsActiveLayout()).toBe("calendar");
          await expect
            .poll(() => moduleLayout(seed, projectId, moduleId, session), { timeout: 30_000 })
            .toBe("calendar");
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );

  test(
    specTitle(["MOD-032"], "small screens open analytics from the bar"),
    { tag: specTags(["MOD-032"]) },
    async ({ driver, seed }) => {
      // This test pins the member leg; guests on small screens are not
      // exercised here (desktop guest legs live in the scenario above).
      const tag = `NF310 mobileanalytics ${Date.now()}`;
      const session = await signInSession(seed.email, seed.password);
      const projectId = await createModuleProject(seed, `${tag} project`, session);
      const moduleName = `${tag} module`;
      const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, moduleName, session);
      const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} issue`);
      await serverAddIssuesToModule(seed.workspaceSlug, projectId, moduleId, [issueId], session);
      try {
        await openDetail(driver, seed, projectId, moduleId, moduleName, session);

        await test.step("members open analytics from the bar", async () => {
          await driver.modulesDetailMobileOpenAnalytics();
          expect(await driver.modulesDetailAnalyticsDialogVisible()).toBe(true);
          expect(await driver.analyticsDialogText()).toContain(moduleName);
          await driver.modulesDetailDismissAnalytics();
          expect(await driver.modulesDetailAnalyticsDialogVisible()).toBe(false);
        });
      } finally {
        await serverCleanupProject(seed.workspaceSlug, projectId, session);
      }
    }
  );
});
