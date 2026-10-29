// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-306): modules list frame and header controls.
// Rows MOD-001 (list, breadcrumb, tab title), MOD-002 (flag gate),
// MOD-003 (Add action), MOD-004 (expandable search), MOD-005 (ordering),
// MOD-006 (filters menu), MOD-007 (applied-filter chips). Green on apps/web
// first.
//
// Fixture shape: every scenario mints its own owner, workspace, and project
// (display choices persist per project). Modules are created through the
// API in-spec; the seed carries none.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  archivesArchiveModule,
  archivesPatchModule,
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  modulesFavoriteModule,
  seatFreshMember,
  serverAddIssuesToModule,
  serverCreateIssue,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverProjectModules,
  serverProjectStates,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_001 = ["MOD-001"];
const ROW_002 = ["MOD-002"];
const ROW_003 = ["MOD-003"];
const ROW_004 = ["MOD-004"];
const ROW_005 = ["MOD-005"];
const ROW_006 = ["MOD-006"];
const ROW_007 = ["MOD-007"];

interface ModulesWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
  projectName: string;
}

/** Fresh owner, workspace, and project for one scenario. */
async function setupModulesWorld(prefix: string, moduleView = true): Promise<ModulesWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack
  // (roughly one setup in eight during development); one retry with a
  // fresh suffix rides it out, and a systematic failure still fails twice.
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ModHead WS ${suffix}`,
        slug: `modh-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectName = `ModHead Proj ${suffix.slice(-6)}`;
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        projectName,
        `MH${suffix.slice(-6)}`.toUpperCase().replace(/[^A-Z0-9]/g, "X"),
        { moduleView },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId, projectName };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Enter the live modules list authenticated as `session`. */
async function enterModulesList(driver: ParityDriver, world: ModulesWorld, session: AuthedSession): Promise<void> {
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
}

/** Seat a workspace member and add them to the project at `role`. */
async function seatProjectMember(world: ModulesWorld, role: number, prefix: string): Promise<AuthedSession> {
  const member = await seatFreshMember(world.owner, world.workspaceSlug, role, prefix);
  await markOnboardedForProjects(member);
  await setLastWorkspaceForProjects(member, world.workspaceId);
  await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
    { member_id: member.userId, role },
  ]);
  return member;
}

test(
  specTitle(ROW_001, "modules list shows every non-archived module with breadcrumb and tab title"),
  { tag: specTags(ROW_001) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod001");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Frame Bravo", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Frame Alpha", world.owner.cookie);
    const archivedId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Frame Archived",
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      archivedId,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, archivedId, world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Frame Alpha");

    await test.step("the list shows every non-archived module", async () => {
      expect(await driver.modulesRowNames()).toEqual(["Frame Alpha", "Frame Bravo"]);
      const serverNames = (await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).map(
        (row) => row.name
      );
      expect(serverNames).toContain("Frame Alpha");
      expect(serverNames).toContain("Frame Bravo");
      expect(serverNames).not.toContain("Frame Archived");
    });

    await test.step("the header shows the project breadcrumb plus a trailing modules crumb", async () => {
      const crumbs = await driver.modulesBreadcrumb();
      expect(crumbs.project).toContain(world.projectName);
      expect(crumbs.trail.length).toBeGreaterThan(0);
      expect(crumbs.trail[crumbs.trail.length - 1]).toBe("Modules");
    });

    await test.step("the tab title carries the project name followed by modules", async () => {
      expect(await driver.modulesPageTitle()).toBe(`${world.projectName} - Modules`);
    });
  }
);

test(
  specTitle(ROW_002, "disabled modules show the flag gate with a settings shortcut"),
  { tag: specTags(ROW_002) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod002", false);
    const member = await seatProjectMember(world, ROLE.MEMBER, "parity-mod002m");

    await test.step("an admin sees art, an explanation, and an enabled shortcut", async () => {
      await enterModulesList(driver, world, world.owner);
      const gate = await driver.modulesGateFacts();
      expect(gate.explanation).toContain("not enabled");
      expect(gate.buttonLabel).toBe("Manage features");
      expect(gate.buttonDisabled).toBe(false);
      expect(gate.hasArt).toBe(true);
      expect(await driver.modulesRowNames()).toEqual([]);
      expect(await driver.modulesChipsVisible()).toBe(false);
    });

    await test.step("the shortcut navigates to the project features settings", async () => {
      await driver.modulesGateToFeatures();
      if (driver.target === "web_new") {
        // NEWFRONT-247: the bare features/ index renders nothing on
        // either app (only the per-feature leaves exist), so the new
        // gate links the modules leaf directly.
        expect(await driver.currentUrlPath()).toBe(
          `/${world.workspaceSlug}/settings/projects/${world.projectId}/features/modules`
        );
      } else {
        expect(await driver.currentUrlPath()).toBe(
          `/${world.workspaceSlug}/settings/projects/${world.projectId}/features/`
        );
      }
    });

    await test.step("a non-admin sees the shortcut disabled", async () => {
      await enterModulesList(driver, world, member);
      const gate = await driver.modulesGateFacts();
      expect(gate.buttonLabel).toBe("Manage features");
      expect(gate.buttonDisabled).toBe(true);
    });
  }
);

test(
  specTitle(ROW_003, "header Add action reads per viewport, opens the dialog, hides for guests"),
  { tag: specTags(ROW_003) },
  async ({ page, driver }) => {
    const world = await setupModulesWorld("parity-mod003");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Add Probe", world.owner.cookie);
    const guest = await seatProjectMember(world, ROLE.GUEST, "parity-mod003g");

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Add Probe");

    await test.step("the button reads Add Module on larger screens", async () => {
      expect(await driver.modulesAddLabel()).toBe("Add Module");
    });

    await test.step("the button reads Add on small screens", async () => {
      await page.setViewportSize({ width: 500, height: 800 });
      expect(await driver.modulesAddLabel()).toBe("Add");
      await page.setViewportSize({ width: 1280, height: 800 });
      expect(await driver.modulesAddLabel()).toBe("Add Module");
    });

    await test.step("activating it opens the create dialog", async () => {
      expect(await driver.modulesCreateDialogOpen()).toBe(false);
      await driver.modulesClickAdd();
      await expect.poll(() => driver.modulesCreateDialogOpen()).toBe(true);
      await page.keyboard.press("Escape");
      await expect.poll(() => driver.modulesCreateDialogOpen()).toBe(false);
    });

    await test.step("guests see no button", async () => {
      await enterModulesList(driver, world, guest);
      await driver.modulesAwaitRow("Add Probe");
      expect(await driver.modulesAddLabel()).toBeNull();
    });
  }
);

test(
  specTitle(ROW_004, "expandable search filters live with an Escape and outside-click matrix"),
  { tag: specTags(ROW_004) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod004");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Search Alpha", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Search Bravo", world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Search Alpha");

    await test.step("the field starts collapsed and opening focuses the text box", async () => {
      expect(await driver.modulesSearchVisible()).toBe(false);
      await driver.modulesOpenSearch();
      expect(await driver.modulesSearchVisible()).toBe(true);
      expect(await driver.modulesSearchFocused()).toBe(true);
    });

    await test.step("typing filters the list by name substring", async () => {
      await driver.modulesTypeSearch("Alpha");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Search Alpha"]);
    });

    await test.step("Escape clears the query first and collapses the empty field on a second press", async () => {
      await driver.modulesEscapeSearch();
      expect(await driver.modulesSearchText()).toBe("");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Search Alpha", "Search Bravo"]);
      expect(await driver.modulesSearchVisible()).toBe(true);
      await driver.modulesEscapeSearch();
      // Collapse animates shut; poll past the transition.
      await expect.poll(() => driver.modulesSearchVisible()).toBe(false);
    });

    await test.step("clicking outside collapses an empty field but keeps a filled one", async () => {
      await driver.modulesOpenSearch();
      await driver.modulesClickOutsideSearch();
      await expect.poll(() => driver.modulesSearchVisible()).toBe(false);
      await driver.modulesOpenSearch();
      await driver.modulesTypeSearch("Bravo");
      await driver.modulesClickOutsideSearch();
      expect(await driver.modulesSearchVisible()).toBe(true);
      expect(await driver.modulesSearchText()).toBe("Bravo");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Search Bravo"]);
      await driver.modulesEscapeSearch();
      await driver.modulesEscapeSearch();
      await expect.poll(() => driver.modulesSearchVisible()).toBe(false);
    });
  }
);

test(
  specTitle(ROW_005, "ordering reorders across keys with a direction toggle except manual"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod005");
    // Creation order (Charlie, Alpha, Bravo) deliberately differs from
    // name order so the created-date key proves a second ordering.
    const charlieId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Order Charlie",
      world.owner.cookie
    );
    const alphaId = await serverCreateModule(world.workspaceSlug, world.projectId, "Order Alpha", world.owner.cookie);
    const bravoId = await serverCreateModule(world.workspaceSlug, world.projectId, "Order Bravo", world.owner.cookie);
    // Due dates run opposite to name order so the key proves a reorder.
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      charlieId,
      { target_date: "2026-12-10", start_date: "2026-11-01" },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      bravoId,
      { target_date: "2026-12-15", start_date: "2026-11-01" },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      alphaId,
      { target_date: "2026-12-20", start_date: "2026-11-01" },
      world.owner.cookie
    );
    // Work-item shape: Alpha holds one done issue; Bravo holds three with
    // one done; Charlie holds two open issues. Counts order Alpha,
    // Charlie, Bravo; progress orders Charlie, Bravo, Alpha — each
    // distinct from name order.
    const states = await serverProjectStates(world.workspaceSlug, world.projectId, world.owner.cookie);
    const doneState = states.find((state) => state.group === "completed");
    if (!doneState) throw new Error("[parity] seed project carries no completed-group state.");
    const alphaDone = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Alpha done",
      doneState.id
    );
    await serverAddIssuesToModule(world.workspaceSlug, world.projectId, alphaId, [alphaDone], world.owner.cookie);
    const bravoOpen = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Bravo open one"
    );
    const bravoOpenTwo = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Bravo open two"
    );
    const bravoDone = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Bravo done",
      doneState.id
    );
    await serverAddIssuesToModule(
      world.workspaceSlug,
      world.projectId,
      bravoId,
      [bravoOpen, bravoOpenTwo, bravoDone],
      world.owner.cookie
    );
    const charlieOpenOne = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Charlie open one"
    );
    const charlieOpenTwo = await serverCreateIssue(
      world.workspaceSlug,
      world.projectId,
      world.owner.cookie,
      "Order Charlie open two"
    );
    await serverAddIssuesToModule(
      world.workspaceSlug,
      world.projectId,
      charlieId,
      [charlieOpenOne, charlieOpenTwo],
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Order Alpha");

    await test.step("the default order is by name ascending", async () => {
      expect(await driver.modulesSortTriggerLabel()).toBe("Name");
      expect(await driver.modulesRowNames()).toEqual(["Order Alpha", "Order Bravo", "Order Charlie"]);
    });

    await test.step("due-date ordering follows the saved dates", async () => {
      await driver.modulesSetSortViaMenu("Due date");
      expect(await driver.modulesSortTriggerLabel()).toBe("Due date");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
    });

    await test.step("created-date ordering follows creation order", async () => {
      await driver.modulesSetSortViaMenu("Created date");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Charlie", "Order Alpha", "Order Bravo"]);
    });

    await test.step("work-item-count ordering follows the hosted totals", async () => {
      await driver.modulesSetSortViaMenu("Number of work items");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Alpha", "Order Charlie", "Order Bravo"]);
    });

    await test.step("progress ordering follows completion share", async () => {
      await driver.modulesSetSortViaMenu("Progress");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
    });

    await test.step("descending reverses the current ordering", async () => {
      await driver.modulesSetSortViaMenu("Name");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Alpha", "Order Bravo", "Order Charlie"]);
      await driver.modulesSetSortViaMenu("Descending");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Charlie", "Order Bravo", "Order Alpha"]);
      expect(await driver.modulesSortTriggerLabel()).toBe("Name");
      await driver.modulesSetSortViaMenu("Ascending");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Order Alpha", "Order Bravo", "Order Charlie"]);
    });

    await test.step("manual placement offers no direction", async () => {
      await driver.modulesSetSortViaMenu("Manual");
      expect(await driver.modulesSortTriggerLabel()).toBe("Manual");
      expect(await driver.modulesSortMenuItems()).toEqual([
        "Name",
        "Progress",
        "Number of work items",
        "Due date",
        "Created date",
        "Manual",
      ]);
    });
  }
);

test(
  specTitle(ROW_005, "manual placement gates timeline drag-reorder"),
  { tag: specTags(ROW_005) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod005t");
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, "Time Alpha", world.owner.cookie);
    const secondId = await serverCreateModule(world.workspaceSlug, world.projectId, "Time Bravo", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      firstId,
      { start_date: "2026-11-01", target_date: "2026-11-20" },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      secondId,
      { start_date: "2026-11-05", target_date: "2026-11-25" },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Time Alpha");
    await driver.modulesSetLayout("timeline");

    await test.step("a drag under manual placement raises no warning and reorders nothing", async () => {
      await driver.modulesSetSortViaMenu("Manual");
      const before = await driver.modulesTimelineRowNames();
      // Manual placement carries the server's stored order rather than
      // name order; the gesture below must preserve whatever it is.
      expect([...before].sort()).toEqual(["Time Alpha", "Time Bravo"]);
      await driver.modulesDragTimelineRowToSelf(before[0] ?? "Time Alpha");
      expect(await driver.modulesManualSortWarningVisible()).toBe(false);
      expect(await driver.modulesTimelineRowNames()).toEqual(before);
    });

    await test.step("the same drag under name order warns that manual is required", async () => {
      await driver.modulesSetSortViaMenu("Name");
      await driver.modulesDragTimelineRowToSelf("Time Alpha");
      await expect.poll(() => driver.modulesManualSortWarningVisible()).toBe(true);
    });
  }
);

test(
  specTitle(ROW_006, "filters menu covers favorites and the six lifecycle statuses"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod006s");
    const favId = await serverCreateModule(world.workspaceSlug, world.projectId, "Filter Fav", world.owner.cookie);
    const doneId = await serverCreateModule(world.workspaceSlug, world.projectId, "Filter Done", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Filter Open", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      doneId,
      { status: "completed" },
      world.owner.cookie
    );
    await modulesFavoriteModule(world.workspaceSlug, world.projectId, favId, world.owner.cookie);

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Filter Fav");

    await test.step("the menu groups span status, lead, members, and dates", async () => {
      expect(await driver.modulesFiltersActive()).toBe(false);
      await driver.modulesOpenFilters();
      expect(await driver.modulesFilterGroups()).toEqual(["Status", "Lead", "Members", "Start date", "Due date"]);
      expect(await driver.modulesFilterGroupOptions("Status")).toEqual([
        "Backlog",
        "Planned",
        "In Progress",
        "Paused",
        "Completed",
        "Cancelled",
      ]);
    });

    await test.step("the favorites toggle narrows to favorited modules", async () => {
      await driver.modulesToggleFilterOption("Favorites", "Favorites");
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Filter Fav"]);
      expect(await driver.modulesFiltersActive()).toBe(true);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["Modules"]);
    });

    await test.step("a status filter narrows to that lifecycle state", async () => {
      await driver.modulesClearFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Filter Done", "Filter Fav", "Filter Open"]);
      await driver.modulesOpenFilters();
      await driver.modulesToggleFilterOption("Status", "Completed");
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Filter Done"]);
      const chips = await driver.modulesChips();
      expect(chips.map((chip) => chip.key)).toEqual(["status"]);
      expect(chips[0]?.text ?? "").toContain("Completed");
      await driver.modulesClearFilters();
    });
  }
);

test(
  specTitle(ROW_006, "lead and member filters order the current user first with a view-all expander"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod006m");
    const leadedId = await serverCreateModule(world.workspaceSlug, world.projectId, "Lead Owned", world.owner.cookie);
    const teamedId = await serverCreateModule(world.workspaceSlug, world.projectId, "Team Shared", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Lead Open", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      leadedId,
      { lead_id: world.owner.userId },
      world.owner.cookie
    );
    // Six extra workspace members push both member groups past five.
    const extras: AuthedSession[] = [];
    for (let i = 0; i < 6; i++) {
      extras.push(await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, `parity-mod006x${i}`));
    }
    const teammate = extras[0];
    if (!teammate) throw new Error("[parity] member seating produced no users.");
    // Module members must belong to the project; the server silently drops
    // workspace-only ids from the patch.
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: teammate.userId, role: ROLE.MEMBER },
    ]);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      teamedId,
      { member_ids: [teammate.userId] },
      world.owner.cookie
    );
    const teammateLabel = teammate.email.split("@")[0] ?? "";

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Lead Owned");

    await test.step("the lead group reads the current user first with a view-all expander", async () => {
      await driver.modulesOpenFilters();
      const firstFive = await driver.modulesFilterGroupOptions("Lead");
      expect(firstFive).toHaveLength(5);
      expect(firstFive[0]).toBe("You");
      expect(await driver.modulesFilterGroupExpandable("Lead")).toBe(true);
      await driver.modulesExpandFilterGroup("Lead");
      expect(await driver.modulesFilterGroupOptions("Lead")).toHaveLength(7);
    });

    await test.step("a lead filter narrows the rows and raises a chip", async () => {
      await driver.modulesToggleFilterOption("Lead", "You");
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Lead Owned"]);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["lead"]);
      await driver.modulesClearFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Lead Open", "Lead Owned", "Team Shared"]);
    });

    await test.step("a member filter narrows to modules carrying that member", async () => {
      await driver.modulesOpenFilters();
      expect(await driver.modulesFilterGroupExpandable("Members")).toBe(true);
      await driver.modulesExpandFilterGroup("Members");
      await driver.modulesToggleFilterOption("Members", teammateLabel);
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Team Shared"]);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["members"]);
      await driver.modulesClearFilters();
    });

    await test.step("the menu search narrows the offered options", async () => {
      await driver.modulesOpenFilters();
      await driver.modulesMenuSearch("cancel");
      expect(await driver.modulesFilterGroupOptions("Status")).toEqual(["Cancelled"]);
      await driver.modulesMenuSearch("");
      expect(await driver.modulesFilterGroupOptions("Status")).toHaveLength(6);
      await driver.modulesCloseFilters();
    });
  }
);

test(
  specTitle(ROW_006, "start and due date filters offer relative options plus a custom range"),
  { tag: specTags(ROW_006) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod006d");
    const futureId = await serverCreateModule(world.workspaceSlug, world.projectId, "Range Future", world.owner.cookie);
    const pastId = await serverCreateModule(world.workspaceSlug, world.projectId, "Range Past", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Range Undated", world.owner.cookie);
    // Range Future starts ten days out (inside the relative start window
    // of "1 week from now") and ends fifteen days out (inside the custom
    // dialog's default today-through-next-month range); start never
    // exceeds target or the server rejects the patch.
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      futureId,
      { start_date: isoDayFromNow(10), target_date: isoDayFromNow(15) },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      pastId,
      { start_date: "2026-01-01", target_date: isoDayFromNow(60) },
      world.owner.cookie
    );
    // Range Undated keeps the null dates modules are born with (the server
    // rejects explicit nulls on PATCH).

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Range Future");

    await test.step("a relative start-date option narrows to matching modules", async () => {
      await driver.modulesOpenFilters();
      expect(await driver.modulesFilterGroupOptions("Start date")).toEqual([
        "1 week from now",
        "2 weeks from now",
        "1 month from now",
        "2 months from now",
        "Custom",
      ]);
      await driver.modulesToggleFilterOption("Start date", "1 week from now");
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Range Future"]);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["start date"]);
      await driver.modulesClearFilters();
    });

    await test.step("the custom due-date dialog applies a range and raises a chip", async () => {
      await driver.modulesOpenFilters();
      await driver.modulesOpenCustomDate("Due date");
      expect(await driver.modulesCustomDateOpen()).toBe(true);
      await driver.modulesApplyCustomDate();
      expect(await driver.modulesCustomDateOpen()).toBe(false);
      await driver.modulesCloseFilters();
      // The default range spans today through next month, keeping only
      // the near-dated module.
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Range Future"]);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["target date"]);
    });

    await test.step("selections combine with the name search", async () => {
      await driver.modulesOpenSearch();
      await driver.modulesTypeSearch("Range P");
      await expect.poll(() => driver.modulesRowNames()).toEqual([]);
      await driver.modulesEscapeSearch();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Range Future"]);
      await driver.modulesClearFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Range Future", "Range Past", "Range Undated"]);
    });
  }
);

test(
  specTitle(ROW_007, "applied-filter chips remove per value, per dimension, and clear-all"),
  { tag: specTags(ROW_007) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod007");
    const firstId = await serverCreateModule(world.workspaceSlug, world.projectId, "Chip Alpha", world.owner.cookie);
    const secondId = await serverCreateModule(world.workspaceSlug, world.projectId, "Chip Bravo", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Chip Gamma", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      firstId,
      { status: "completed", lead_id: world.owner.userId },
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      secondId,
      { status: "planned" },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner);
    await driver.modulesAwaitRow("Chip Alpha");

    await test.step("each active dimension raises one chip with its values", async () => {
      expect(await driver.modulesChipsVisible()).toBe(false);
      await driver.modulesOpenFilters();
      await driver.modulesToggleFilterOption("Status", "Completed");
      await driver.modulesToggleFilterOption("Status", "Planned");
      await driver.modulesToggleFilterOption("Lead", "You");
      await driver.modulesCloseFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Chip Alpha"]);
      expect(await driver.modulesChipsVisible()).toBe(true);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["status", "lead"]);
    });

    await test.step("one value removes without touching its siblings", async () => {
      await driver.modulesRemoveChipValue("status", "Planned");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Chip Alpha"]);
      const chips = await driver.modulesChips();
      expect(chips.map((chip) => chip.key)).toEqual(["status", "lead"]);
      expect(chips[0]?.text ?? "").toContain("Completed");
      expect(chips[0]?.text ?? "").not.toContain("Planned");
    });

    await test.step("one dimension removes without touching the others", async () => {
      await driver.modulesRemoveChipDimension("lead");
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Chip Alpha"]);
      expect((await driver.modulesChips()).map((chip) => chip.key)).toEqual(["status"]);
    });

    await test.step("clear-all resets everything and hides the strip", async () => {
      await driver.modulesClearFilters();
      await expect.poll(() => driver.modulesRowNames()).toEqual(["Chip Alpha", "Chip Bravo", "Chip Gamma"]);
      expect(await driver.modulesChips()).toEqual([]);
      expect(await driver.modulesChipsVisible()).toBe(false);
      expect(await driver.modulesFiltersActive()).toBe(false);
    });
  }
);

/** ISO day `days` from today (custom-range and relative-date fixtures). */
function isoDayFromNow(days: number): string {
  return new Date(Date.now() + days * 86_400_000).toISOString().slice(0, 10);
}
