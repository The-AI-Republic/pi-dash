// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-225): archived modules. Rows ARCH-020 (rows,
// peek, sort), ARCH-021 (search, filters, chips), ARCH-022 (empty and
// no-match states, skeleton), ARCH-023 (archive dialog), ARCH-024
// (restore from the row menu), ARCH-025 (read-only archived peek panels
// for modules and cycles). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner, workspace, and
// project (module and cycle views on), because sort, filter, and display
// choices persist per project. Modules and cycles are created and
// archived through the API in-spec; the seed carries none.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  archivesArchiveCycle,
  archivesArchiveModule,
  archivesArchivedCycles,
  archivesArchivedModules,
  archivesModuleIsArchived,
  archivesPatchModule,
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  seatFreshMember,
  serverCreateCycle,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverProjectModules,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_020 = ["ARCH-020"];
const ROW_021 = ["ARCH-021"];
const ROW_022 = ["ARCH-022"];
const ROW_023 = ["ARCH-023"];
const ROW_024 = ["ARCH-024"];
const ROW_025 = ["ARCH-025"];

interface ArchivesWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
}

/** Fresh owner, workspace, and views-enabled project for one scenario. */
async function setupArchivesWorld(prefix: string): Promise<ArchivesWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack
  // (roughly one setup in eight during development); one retry with a
  // fresh suffix rides it out, and a systematic failure still fails twice.
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ArchMod WS ${suffix}`,
        slug: `archm-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        "Archive Modules",
        `AM${suffix.slice(-8)}`.toUpperCase(),
        { moduleView: true, cycleView: true },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Create a module, move it to `status`, and archive it; resolves its id. */
async function createArchivedModule(
  world: ArchivesWorld,
  name: string,
  status: "completed" | "cancelled"
): Promise<string> {
  const id = await serverCreateModule(world.workspaceSlug, world.projectId, name, world.owner.cookie);
  await archivesPatchModule(world.workspaceSlug, world.projectId, id, { status }, world.owner.cookie);
  await archivesArchiveModule(world.workspaceSlug, world.projectId, id, world.owner.cookie);
  return id;
}

/**
 * Enter the archived-modules tab authenticated as `session`. A direct
 * load hangs on the skeleton (NEWFRONT-228), so entry always goes
 * through the live screen plus client-side navigation.
 */
async function enterArchivesModules(driver: ParityDriver, world: ArchivesWorld, session: AuthedSession): Promise<void> {
  // The live open skips its reload when already there, so each screen
  // below loads exactly once.
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
  await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
}

/** Same live-first client-side entry for the archived-cycles tab. */
async function enterArchivesCycles(driver: ParityDriver, world: ArchivesWorld, session: AuthedSession): Promise<void> {
  // The live open skips its reload when already there, so each screen
  // below loads exactly once.
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/cycles`,
    browserSessionCookies(session)
  );
  await driver.archivesOpenCyclesTab(world.workspaceSlug, world.projectId);
}

test(
  specTitle(ROW_020, "archived modules list reorders through the sort control"),
  { tag: specTags(ROW_020) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch020");
    // Creation order (Charlie, Alpha, Bravo) deliberately differs from
    // name order so the created-date ordering proves a second key.
    await createArchivedModule(world, "Sort Charlie", "completed");
    await createArchivedModule(world, "Sort Alpha", "completed");
    await createArchivedModule(world, "Sort Bravo", "cancelled");
    expect(await archivesArchivedModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toHaveLength(3);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Sort Alpha");

    await test.step("default order is by name ascending", async () => {
      expect(await driver.archivesModuleSortLabel()).toBe("Name");
      expect(await driver.archivesModuleRowNames()).toEqual(["Sort Alpha", "Sort Bravo", "Sort Charlie"]);
    });

    await test.step("created-date ordering follows creation order", async () => {
      await driver.archivesSetModuleSort("Created date");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Sort Charlie", "Sort Alpha", "Sort Bravo"]);
    });

    await test.step("descending reverses the current ordering", async () => {
      await driver.archivesSetModuleSort("Descending");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Sort Bravo", "Sort Alpha", "Sort Charlie"]);
    });
  }
);

test(
  specTitle(ROW_020, "selecting an archived module opens its peek panel from the detail read"),
  { tag: specTags(ROW_020) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch020p");
    await createArchivedModule(world, "Peek Module", "completed");

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Peek Module");

    await test.step("clicking the row opens the peek with the archived record", async () => {
      expect(await driver.archivesModulePeekName()).toBeNull();
      // The driver waits for the archived-module detail fetch, so reaching
      // the name assertion proves the peek loaded through that read.
      await driver.archivesOpenModulePeek("Peek Module");
      expect(await driver.archivesModulePeekName()).toBe("Peek Module");
      expect(await driver.currentPath()).toContain("peekModule=");
    });

    await test.step("closing the peek clears the address selection", async () => {
      await driver.archivesCloseModulePeek();
      expect(await driver.currentPath()).not.toContain("peekModule=");
      expect(await driver.archivesModulePeekName()).toBeNull();
    });
  }
);

test(
  specTitle(ROW_021, "archived modules search expands, filters live, and collapses"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch021s");
    await createArchivedModule(world, "Search Alpha", "completed");
    await createArchivedModule(world, "Search Bravo", "cancelled");

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Search Alpha");

    await test.step("the box starts collapsed and opens through the magnifier", async () => {
      expect(await driver.archivesModuleSearchVisible()).toBe(false);
      await driver.archivesOpenModuleSearch();
      expect(await driver.archivesModuleSearchVisible()).toBe(true);
    });

    await test.step("typing filters the rows live", async () => {
      await driver.archivesTypeModuleSearch("Alpha");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Search Alpha"]);
    });

    await test.step("Escape clears the text first and collapses on the second press", async () => {
      await driver.archivesEscapeModuleSearch();
      expect(await driver.archivesModuleSearchText()).toBe("");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Search Alpha", "Search Bravo"]);
      expect(await driver.archivesModuleSearchVisible()).toBe(true);
      await driver.archivesEscapeModuleSearch();
      // Collapse animates shut; poll past the transition.
      await expect.poll(() => driver.archivesModuleSearchVisible()).toBe(false);
    });

    await test.step("the clear button empties and collapses", async () => {
      await driver.archivesOpenModuleSearch();
      await driver.archivesTypeModuleSearch("Bravo");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Search Bravo"]);
      await driver.archivesClearModuleSearch();
      expect(await driver.archivesModuleSearchText()).toBe("");
      await expect.poll(() => driver.archivesModuleSearchVisible()).toBe(false);
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Search Alpha", "Search Bravo"]);
    });

    await test.step("clicking away collapses an empty box and keeps a filled one", async () => {
      await driver.archivesOpenModuleSearch();
      await driver.archivesCollapseSearchOutside();
      await expect.poll(() => driver.archivesModuleSearchVisible()).toBe(false);
      await driver.archivesOpenModuleSearch();
      await driver.archivesTypeModuleSearch("Alpha");
      await driver.archivesCollapseSearchOutside();
      expect(await driver.archivesModuleSearchVisible()).toBe(true);
      expect(await driver.archivesModuleSearchText()).toBe("Alpha");
      await driver.archivesClearModuleSearch();
    });
  }
);

test(
  specTitle(ROW_021, "archived modules filters narrow the list with removable chips"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch021f");
    const leaded = await serverCreateModule(world.workspaceSlug, world.projectId, "Lead Owned", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      leaded,
      { status: "completed", lead_id: world.owner.userId },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, leaded, world.owner.cookie);
    await createArchivedModule(world, "Lead Open", "cancelled");

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Lead Owned");

    await test.step("the filters menu covers lead, members, and dates", async () => {
      await driver.archivesOpenModuleFilters();
      expect(await driver.archivesModuleFilterGroups()).toEqual(["Lead", "Members", "Start date", "Due date"]);
    });

    await test.step("a lead filter narrows the rows and raises a chip", async () => {
      expect(await driver.archivesModuleFiltersActive()).toBe(false);
      // The current user reads as "You" among the lead options.
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Lead Owned"]);
      const chips = await driver.archivesModuleChips();
      expect(chips.map((chip) => chip.key)).toEqual(["lead"]);
      // The menu button itself never indicates (NEWFRONT-229, pinned by
      // the bug scenario below); the chip row is the applied signal.
    });

    await test.step("removing the chip restores the list", async () => {
      await driver.archivesRemoveModuleChip("lead");
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Lead Open", "Lead Owned"]);
      expect(await driver.archivesModuleChips()).toEqual([]);
      expect(await driver.archivesModuleFiltersActive()).toBe(false);
    });

    await test.step("clear-all drops every applied filter", async () => {
      await driver.archivesOpenModuleFilters();
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Lead Owned"]);
      await driver.archivesClearModuleFilters();
      await expect.poll(() => driver.archivesModuleRowNames()).toEqual(["Lead Open", "Lead Owned"]);
      expect(await driver.archivesModuleFiltersActive()).toBe(false);
    });
  }
);

test(
  specTitle(ROW_021, "bug: NEWFRONT-229 filters button shows no indication with filters applied"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch021b");
    const leaded = await serverCreateModule(world.workspaceSlug, world.projectId, "Dot Owned", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      leaded,
      { status: "completed", lead_id: world.owner.userId },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, leaded, world.owner.cookie);

    await enterArchivesModules(driver, world, world.owner);

    // The filter applies (chip renders, list narrows) but the menu
    // button's wide variant never displays, so no dot ever shows.
    await driver.archivesOpenModuleFilters();
    await driver.archivesToggleLeadFilter("You");
    await driver.archivesCloseModuleFilters();
    expect(await driver.archivesModuleChips()).not.toEqual([]);
    expect(await driver.archivesModuleFiltersActive()).toBe(false);
  }
);

test(
  specTitle(ROW_022, "archived modules empty, no-match, and loading states"),
  { tag: specTags(ROW_022) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch022");

    await enterArchivesModules(driver, world, world.owner);

    await test.step("zero archived modules shows the empty illustration", async () => {
      expect(await driver.archivesModulesEmptyKind()).toBe("zero");
      expect(await archivesArchivedModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await createArchivedModule(world, "State Alpha", "completed");
    await createArchivedModule(world, "State Bravo", "cancelled");
    await driver.archivesOpenModulesTab(world.workspaceSlug, world.projectId);
    await driver.archivesAwaitModuleRow("State Alpha");

    await test.step("filters hiding everything show the filters no-match state", async () => {
      // No module names the owner as lead, so filtering to "You" empties the list.
      await driver.archivesOpenModuleFilters();
      await driver.archivesToggleLeadFilter("You");
      await driver.archivesCloseModuleFilters();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("filters");
      await driver.archivesClearModuleFilters();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("rows");
    });

    await test.step("search hiding everything shows the search no-match state", async () => {
      await driver.archivesOpenModuleSearch();
      await driver.archivesTypeModuleSearch("no-such-module-zzz");
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("search");
      await driver.archivesClearModuleSearch();
      await expect.poll(() => driver.archivesModulesEmptyKind()).toBe("rows");
    });

    await test.step("a skeleton loader shows while the list fetches", async () => {
      expect(await driver.archivesModulesShowsSkeleton(world.workspaceSlug, world.projectId)).toBe(true);
    });
  }
);

test(
  specTitle(ROW_023, "archive entry renders for editors and gates on finished status"),
  { tag: specTags(ROW_023) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch023g");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Gate Planned", world.owner.cookie);
    const doneId = await serverCreateModule(world.workspaceSlug, world.projectId, "Gate Done", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      doneId,
      { status: "completed" },
      world.owner.cookie
    );
    await createArchivedModule(world, "Gate Archived", "completed");
    const guest = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.GUEST, "parity-arch023g");
    await markOnboardedForProjects(guest);
    await setLastWorkspaceForProjects(guest, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: guest.userId, role: ROLE.GUEST },
    ]);

    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
      browserSessionCookies(world.owner)
    );
    await driver.archivesAwaitModuleRow("Gate Planned");

    await test.step("an unfinished module keeps the entry disabled with an explanation", async () => {
      const entries = await driver.archivesLiveModuleMenuEntries("Gate Planned");
      const archive = entries.find((entry) => entry.title === "Archive");
      expect(archive).toBeDefined();
      expect(archive?.disabled).toBe(true);
      expect(archive?.description ?? "").toContain("completed");
    });

    await test.step("a completed module offers an enabled entry", async () => {
      const entries = await driver.archivesLiveModuleMenuEntries("Gate Done");
      const archive = entries.find((entry) => entry.title === "Archive");
      expect(archive).toBeDefined();
      expect(archive?.disabled).toBe(false);
      expect(archive?.description).toBeNull();
    });

    await test.step("a guest sees no archive entry", async () => {
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
        browserSessionCookies(guest)
      );
      await driver.archivesAwaitModuleRow("Gate Done");
      const entries = await driver.archivesLiveModuleMenuEntries("Gate Done");
      expect(entries.map((entry) => entry.title)).not.toContain("Archive");
    });

    await test.step("an archived module hides the entry", async () => {
      await enterArchivesModules(driver, world, world.owner);
      await driver.archivesAwaitModuleRow("Gate Archived");
      const entries = await driver.archivesArchivedModuleMenuEntries("Gate Archived");
      expect(entries.map((entry) => entry.title)).not.toContain("Archive");
    });
  }
);

test(
  specTitle(ROW_023, "archive dialog confirms, cancels, and reports failure"),
  { tag: specTags(ROW_023) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch023d");
    const confirmed = await serverCreateModule(world.workspaceSlug, world.projectId, "Dialog Yes", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      confirmed,
      { status: "completed" },
      world.owner.cookie
    );
    const cancelled = await serverCreateModule(world.workspaceSlug, world.projectId, "Dialog No", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      cancelled,
      { status: "cancelled" },
      world.owner.cookie
    );
    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
      browserSessionCookies(world.owner)
    );
    await driver.archivesAwaitModuleRow("Dialog Yes");

    await test.step("confirming archives and returns to the live modules screen", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Dialog Yes", "Archive");
      const dialog = await driver.archivesArchiveDialog();
      expect(dialog?.title).toContain("Dialog Yes");
      expect(dialog?.body).toContain("restored");
      await driver.archivesConfirmArchiveDialog();
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Archive success" });
      expect(await driver.currentUrlPath()).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules/`);
      expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, confirmed, world.owner.cookie)).toBe(
        true
      );
      await expect.poll(() => driver.archivesModuleRowNames()).not.toContain("Dialog Yes");
    });

    await test.step("cancelling changes nothing", async () => {
      await driver.archivesChooseLiveModuleMenuEntry("Dialog No", "Archive");
      expect(await driver.archivesArchiveDialog()).not.toBeNull();
      await driver.archivesCancelArchiveDialog();
      expect(await driver.archivesArchiveDialog()).toBeNull();
      expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, cancelled, world.owner.cookie)).toBe(
        false
      );
    });
  }
);

test(
  specTitle(ROW_023, "bug: NEWFRONT-233 a failed archive reports success and navigates anyway"),
  { tag: specTags(ROW_023) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch023b");
    const failing = await serverCreateModule(world.workspaceSlug, world.projectId, "Dialog Fail", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      failing,
      { status: "completed" },
      world.owner.cookie
    );

    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
      browserSessionCookies(world.owner)
    );
    await driver.archivesAwaitModuleRow("Dialog Fail");

    // The store swallows the write error, so the dialog runs its success
    // path: success confirmation, close, navigate — module stays live.
    await driver.archivesFailNextModuleWrite();
    await driver.archivesChooseLiveModuleMenuEntry("Dialog Fail", "Archive");
    await driver.archivesConfirmArchiveDialog();
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Archive success" });
    expect(await driver.archivesArchiveDialog()).toBeNull();
    expect(await driver.currentUrlPath()).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/modules/`);
    expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, failing, world.owner.cookie)).toBe(
      false
    );
    expect(await driver.archivesModuleRowNames()).toContain("Dialog Fail");
  }
);

test(
  specTitle(ROW_024, "restore an archived module from the row menu"),
  { tag: specTags(ROW_024) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch024");
    const restored = await createArchivedModule(world, "Restore Me", "completed");
    const guest = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.GUEST, "parity-arch024g");
    await markOnboardedForProjects(guest);
    await setLastWorkspaceForProjects(guest, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: guest.userId, role: ROLE.GUEST },
    ]);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Restore Me");

    await test.step("editors see restore alongside copy and open, without edit or delete", async () => {
      const entries = await driver.archivesArchivedModuleMenuEntries("Restore Me");
      const titles = entries.map((entry) => entry.title);
      expect(titles).toContain("Restore");
      expect(titles).toContain("Copy link");
      expect(titles).toContain("Open in new tab");
      expect(titles).not.toContain("Edit");
      expect(titles).not.toContain("Delete");
      expect(entries.find((entry) => entry.title === "Restore")?.disabled).toBe(false);
    });

    await test.step("a guest sees no restore entry", async () => {
      // Guests get no sidebar Archives entry, so they enter through a
      // client-side hop after the live visit seeds the store.
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
        browserSessionCookies(guest)
      );
      await driver.archivesOpenLiveModules(world.workspaceSlug, world.projectId);
      await driver.archivesClientNavigate(`/${world.workspaceSlug}/projects/${world.projectId}/archives/modules`);
      await driver.archivesAwaitModuleRow("Restore Me");
      const entries = await driver.archivesArchivedModuleMenuEntries("Restore Me");
      expect(entries.map((entry) => entry.title)).not.toContain("Restore");
      await enterArchivesModules(driver, world, world.owner);
      await driver.archivesAwaitModuleRow("Restore Me");
    });

    await test.step("restoring keeps the user on the archives tab with the module live", async () => {
      await driver.archivesChooseArchivedModuleMenuEntry("Restore Me", "Restore");
      await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Restore success" });
      expect(await driver.currentUrlPath()).toBe(
        `/${world.workspaceSlug}/projects/${world.projectId}/archives/modules/`
      );
      expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, restored, world.owner.cookie)).toBe(
        false
      );
      await expect.poll(() => driver.archivesModuleRowNames()).not.toContain("Restore Me");
      const live = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(live.map((row) => row.name)).toContain("Restore Me");
    });
  }
);

test(
  specTitle(ROW_024, "bug: NEWFRONT-233 a failed restore reports success and keeps the row"),
  { tag: specTags(ROW_024) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch024b");
    const failing = await createArchivedModule(world, "Restore Fail", "cancelled");

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Restore Fail");

    // The store swallows the write error, so the menu runs its success
    // path: success confirmation, same-tab stay — module stays archived.
    await driver.archivesFailNextModuleWrite();
    await driver.archivesChooseArchivedModuleMenuEntry("Restore Fail", "Restore");
    await expect.poll(() => driver.rulesLastToast(), { timeout: 30_000 }).toMatchObject({ title: "Restore success" });
    expect(await driver.currentUrlPath()).toBe(`/${world.workspaceSlug}/projects/${world.projectId}/archives/modules/`);
    expect(await archivesModuleIsArchived(world.workspaceSlug, world.projectId, failing, world.owner.cookie)).toBe(
      true
    );
    expect(await driver.archivesModuleRowNames()).toContain("Restore Fail");
  }
);

test(
  specTitle(ROW_025, "archived module peek panel is read-only next to its live twin"),
  { tag: specTags(ROW_025) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch025m");
    await createArchivedModule(world, "Readonly Module", "completed");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Live Module", world.owner.cookie);

    await enterArchivesModules(driver, world, world.owner);
    await driver.archivesAwaitModuleRow("Readonly Module");

    await test.step("the archived panel shows details with editing locked", async () => {
      await driver.archivesOpenModulePeek("Readonly Module");
      const panel = await driver.archivesModulePeekReadOnly();
      expect(panel.name).toBe("Readonly Module");
      expect(panel.summaryShown).toBe(true);
      expect(panel.statusLocked).toBe(true);
      expect(panel.addLinkOffered).toBe(false);
      await driver.archivesCloseModulePeek();
    });

    await test.step("the live twin offers the editing affordances", async () => {
      await driver.archivesOpenLiveModules(world.workspaceSlug, world.projectId);
      await driver.archivesAwaitModuleRow("Live Module");
      await driver.archivesOpenLiveModulePeek("Live Module");
      const panel = await driver.archivesModulePeekReadOnly();
      expect(panel.name).toBe("Live Module");
      expect(panel.statusLocked).toBe(false);
      expect(panel.addLinkOffered).toBe(true);
      await driver.archivesCloseModulePeek();
    });
  }
);

test(specTitle(ROW_025, "archived cycle peek panel is read-only"), { tag: specTags(ROW_025) }, async ({ driver }) => {
  const world = await setupArchivesWorld("parity-arch025c");
  const cycleId = await serverCreateCycle(
    world.workspaceSlug,
    world.projectId,
    "Readonly Cycle",
    "2020-01-01",
    "2020-01-15",
    world.owner.cookie
  );
  await archivesArchiveCycle(world.workspaceSlug, world.projectId, cycleId, world.owner.cookie);
  expect(
    (await archivesArchivedCycles(world.workspaceSlug, world.projectId, world.owner.cookie)).map((row) => row.name)
  ).toContain("Readonly Cycle");
  // A live cycle keeps the live screen (the entry path) non-empty.
  await serverCreateCycle(
    world.workspaceSlug,
    world.projectId,
    "Live Cycle",
    "2030-01-01",
    "2030-12-31",
    world.owner.cookie
  );

  await enterArchivesCycles(driver, world, world.owner);

  await test.step("the archived cycle panel shows details with editing locked", async () => {
    await expect.poll(() => driver.archivesCycleRowNames()).toContain("Readonly Cycle");
    await driver.archivesOpenCyclePeek("Readonly Cycle");
    const panel = await driver.archivesCyclePeekReadOnly();
    expect(panel.name).toBe("Readonly Cycle");
    expect(panel.summaryShown).toBe(true);
    expect(panel.statusLocked).toBe(true);
    expect(panel.addLinkOffered).toBe(false);
    await driver.archivesCloseCyclePeek();
    expect(await driver.currentPath()).not.toContain("peekCycle=");
  });
});

test(
  specTitle(ROW_020, "bug: NEWFRONT-228 direct load of the archived modules tab never renders"),
  { tag: specTags(ROW_020) },
  async ({ driver }) => {
    const world = await setupArchivesWorld("parity-arch020b");
    await createArchivedModule(world, "Direct Bug", "completed");
    expect(
      (await archivesArchivedModules(world.workspaceSlug, world.projectId, world.owner.cookie)).map((row) => row.name)
    ).toContain("Direct Bug");

    // A fresh session with no prior live visit: the list fetch completes
    // (200 with rows) but the tab stays on the skeleton. Every other
    // scenario enters through the live screen until NEWFRONT-228 is fixed.
    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/archives/modules`,
      browserSessionCookies(world.owner)
    );
    expect(await driver.archivesDirectLoadRenders(world.workspaceSlug, world.projectId)).toBe(false);
  }
);
