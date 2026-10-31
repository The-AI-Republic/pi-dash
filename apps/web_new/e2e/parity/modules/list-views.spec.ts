// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-307): live modules list states — the
// per-layout loading skeleton, the zero-modules onboarding view with its
// member-only creation shortcut, the search no-match view, the persisted
// layout switcher (desktop and small screens), list row identity
// (progress marker, detail link, peek trigger, small-screen menu,
// archived-row peek routing), and board card identity with body-vs-control
// click routing. Rows: MOD-008–MOD-012, MOD-020. Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner, workspace, and
// project (modules view on), because layout and display choices persist
// per project. Modules and their work items are created through the API
// in-spec; the seed carries none.
//
// Scope boundaries (sibling children of NEWFRONT-50): the search field
// and filter menu themselves belong to NEWFRONT-306 (MOD-004/006) — the
// no-match scenario drives search only as a means to empty the list (one
// filtered-empty branch serves every cause). Inline control behavior on
// rows and cards belongs to NEWFRONT-308 (MOD-013/014/016/017) — these
// scenarios assert presence and click routing only. Peek panel content is
// MOD-019 (NEWFRONT-308); here the peek must only open through its
// detail read. The archived-modules screen belongs to NEWFRONT-309
// (MOD-027); the archived-row scenario enters it read-only through the
// merged archives entry helpers to prove click routing alone.
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
  modulesListProgress,
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
import { openArchivesModulesTab } from "./chunkRecovery";

const ROW_008 = ["MOD-008"];
const ROW_009 = ["MOD-009"];
const ROW_010 = ["MOD-010"];
const ROW_011 = ["MOD-011"];
const ROW_012 = ["MOD-012"];
const ROW_020 = ["MOD-020"];

interface ModulesWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
}

/** Fresh owner, workspace, and modules-enabled project for one scenario. */
async function setupModulesWorld(prefix: string): Promise<ModulesWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack;
  // one retry with a fresh suffix rides it out, and a systematic failure
  // still fails twice.
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ModViews WS ${suffix}`,
        slug: `modlv-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        "List Views",
        `LV${suffix.slice(-8)}`.toUpperCase(),
        { moduleView: true },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Enter the live modules list authenticated as `session`. */
async function enterLiveModules(driver: ParityDriver, world: ModulesWorld, session: AuthedSession): Promise<void> {
  await driver.openAuthenticated(
    `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
    browserSessionCookies(session)
  );
  await driver.modulesListOpenList(world.workspaceSlug, world.projectId);
}

/** Poll a module's server counters until they read exactly `want`. */
async function awaitModuleProgress(
  world: ModulesWorld,
  moduleId: string,
  want: { completed: number; cancelled: number; total: number }
): Promise<void> {
  await expect
    .poll(() => modulesListProgress(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie), {
      timeout: 30_000,
    })
    .toEqual(want);
}

/**
 * Seed `total` work items on a module, `done` of them completed; ends
 * with the server counters settled (the list never refreshes on its
 * own, so every read below must already be final).
 */
async function seedModuleIssues(world: ModulesWorld, moduleId: string, total: number, done: number): Promise<void> {
  const states = await serverProjectStates(world.workspaceSlug, world.projectId, world.owner.cookie);
  const completed = states.find((state) => state.group === "completed");
  if (!completed) throw new Error("[parity] project has no completed-group state.");
  const ids: string[] = [];
  for (let i = 0; i < total; i++) {
    ids.push(
      await serverCreateIssue(
        world.workspaceSlug,
        world.projectId,
        world.owner.cookie,
        `NF307 issue ${i}`,
        i < done ? completed.id : undefined
      )
    );
  }
  await serverAddIssuesToModule(world.workspaceSlug, world.projectId, moduleId, ids, world.owner.cookie);
  await awaitModuleProgress(world, moduleId, { completed: done, cancelled: 0, total });
}

test(
  specTitle(ROW_008, "loading skeleton matches the active layout and shows alone"),
  { tag: specTags(ROW_008) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod008");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Skeleton Module", world.owner.cookie);
    await enterLiveModules(driver, world, world.owner);

    for (const layout of ["list", "board", "timeline"] as const) {
      await test.step(`${layout} skeleton shows exclusively while the fetch is held`, async () => {
        const seen = await driver.modulesListSkeletonShownOnSlowFetch(world.workspaceSlug, world.projectId, layout);
        expect(seen).toEqual({ shown: true, layoutMatched: true, exclusive: true });
        // The skeleton gives way to content in the same layout, never to
        // an empty state.
        if (layout === "list") {
          expect(await driver.modulesListVisibleNames()).toEqual(["Skeleton Module"]);
        } else if (layout === "board") {
          expect(await driver.modulesListBoardCardNames()).toEqual(["Skeleton Module"]);
        } else {
          expect(await driver.modulesListTimelineNames()).toEqual(["Skeleton Module"]);
        }
      });
    }
  }
);

test(
  specTitle(ROW_009, "zero modules shows onboarding with a working creation shortcut"),
  { tag: specTags(ROW_009) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod009");
    await enterLiveModules(driver, world, world.owner);

    await test.step("onboarding art, explanation and an enabled creation action render", async () => {
      expect(await driver.modulesListEmptyKind()).toBe("zero");
      expect(await driver.modulesListZeroStateVisible()).toBe(true);
      expect(await driver.modulesListZeroStateArtVisible()).toBe(true);
      const copy = await driver.modulesListZeroStateCopy();
      expect(copy.title).not.toBe("");
      expect(copy.description).not.toBe("");
      expect(await driver.modulesListZeroStateCreateDisabled()).toBe(false);
      expect(await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie)).toEqual([]);
    });

    await test.step("the creation action opens the create dialog", async () => {
      await driver.modulesListZeroStateCreateOpen();
      expect(await driver.modulesListCreateDialogVisible()).toBe(true);
    });
  }
);

test(
  specTitle(ROW_009, "zero-state creation action is disabled for guests"),
  { tag: specTags(ROW_009) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod009g");
    const guest = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.GUEST, "parity-mod009g");
    await markOnboardedForProjects(guest);
    await setLastWorkspaceForProjects(guest, world.workspaceId);
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: guest.userId, role: ROLE.GUEST },
    ]);

    await enterLiveModules(driver, world, guest);

    expect(await driver.modulesListEmptyKind()).toBe("zero");
    expect(await driver.modulesListZeroStateVisible()).toBe(true);
    expect(await driver.modulesListZeroStateCreateDisabled()).toBe(true);
  }
);

test(
  specTitle(ROW_010, "search matching nothing shows the no-match view and keeps every module"),
  { tag: specTags(ROW_010) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod010");
    await serverCreateModule(world.workspaceSlug, world.projectId, "NoMatch Alpha", world.owner.cookie);
    await serverCreateModule(world.workspaceSlug, world.projectId, "NoMatch Bravo", world.owner.cookie);
    await enterLiveModules(driver, world, world.owner);
    expect(await driver.modulesListEmptyKind()).toBe("rows");

    await test.step("a matching-nothing search shows the search hint", async () => {
      await driver.modulesListNoMatchShowViaSearch("zzz-no-such-module");
      expect(await driver.modulesListEmptyKind()).toBe("nomatch");
      expect(await driver.modulesListNoMatchHeading()).not.toBe(null);
      expect(await driver.modulesListNoMatchHint()).toMatch(/adjust/i);
      expect(await driver.modulesListVisibleNames()).toEqual([]);
    });

    await test.step("clearing restores the untouched modules", async () => {
      await driver.modulesListNoMatchClearSearch();
      // Order is MOD-005 (NEWFRONT-306); the row only needs both back.
      expect((await driver.modulesListVisibleNames()).sort()).toEqual(["NoMatch Alpha", "NoMatch Bravo"]);
      const stored = await serverProjectModules(world.workspaceSlug, world.projectId, world.owner.cookie);
      expect(stored.map((row) => row.name).sort()).toEqual(["NoMatch Alpha", "NoMatch Bravo"]);
    });
  }
);

test(
  specTitle(ROW_011, "layout switcher re-renders and persists the choice"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod011");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Layout Module", world.owner.cookie);
    await enterLiveModules(driver, world, world.owner);

    await test.step("the switcher offers three layouts", async () => {
      expect(await driver.modulesListSwitcherVisible()).toBe(true);
      expect(await driver.modulesListLayoutOptionCount()).toBe(3);
      await driver.modulesListSwitchLayout("list");
      expect(await driver.modulesListActiveLayout()).toBe("list");
      expect(await driver.modulesListVisibleNames()).toEqual(["Layout Module"]);
    });

    await test.step("board re-renders as cards and survives reload", async () => {
      await driver.modulesListSwitchLayout("board");
      expect(await driver.modulesListActiveLayout()).toBe("board");
      expect(await driver.modulesListBoardCardNames()).toEqual(["Layout Module"]);
      expect(await driver.modulesListVisibleNames()).toEqual([]);
      await driver.modulesListReload();
      expect(await driver.modulesListActiveLayout()).toBe("board");
      expect(await driver.modulesListBoardCardNames()).toEqual(["Layout Module"]);
    });

    await test.step("timeline re-renders and survives reload, then list restores", async () => {
      await driver.modulesListSwitchLayout("timeline");
      expect(await driver.modulesListActiveLayout()).toBe("timeline");
      expect(await driver.modulesListTimelineNames()).toEqual(["Layout Module"]);
      expect(await driver.modulesListVisibleNames()).toEqual([]);
      expect(await driver.modulesListBoardCardNames()).toEqual([]);
      await driver.modulesListReload();
      expect(await driver.modulesListActiveLayout()).toBe("timeline");
      expect(await driver.modulesListTimelineNames()).toEqual(["Layout Module"]);
      await driver.modulesListSwitchLayout("list");
      expect(await driver.modulesListVisibleNames()).toEqual(["Layout Module"]);
    });
  }
);

test(
  specTitle(ROW_011, "small screens offer list and board through the layout menu"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod011m");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Compact Module", world.owner.cookie);
    await driver.setViewportSize(390, 844);
    await enterLiveModules(driver, world, world.owner);

    await test.step("the desktop switcher hides and the menu offers list and board only", async () => {
      expect(await driver.modulesListSwitcherVisible()).toBe(false);
      expect(await driver.modulesListCompactMenuVisible()).toBe(true);
      expect(await driver.modulesListCompactMenuOptions()).toEqual(["List layout", "Gallery layout"]);
    });

    await test.step("the menu switches between list and board", async () => {
      await driver.modulesListCompactSwitchLayout("board");
      expect(await driver.modulesListBoardCardNames()).toEqual(["Compact Module"]);
      await driver.modulesListCompactSwitchLayout("list");
      expect(await driver.modulesListVisibleNames()).toEqual(["Compact Module"]);
    });
  }
);

test(
  specTitle(ROW_012, "list rows show progress, detail links and peek triggers"),
  { tag: specTags(ROW_012) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod012");
    const percentId = await serverCreateModule(world.workspaceSlug, world.projectId, "Row Percent", world.owner.cookie);
    await seedModuleIssues(world, percentId, 4, 1);
    const checkId = await serverCreateModule(world.workspaceSlug, world.projectId, "Row Check", world.owner.cookie);
    await seedModuleIssues(world, checkId, 2, 2);
    const alertId = await serverCreateModule(world.workspaceSlug, world.projectId, "Row Alert", world.owner.cookie);
    await seedModuleIssues(world, alertId, 2, 0);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      alertId,
      { status: "completed" },
      world.owner.cookie
    );
    await enterLiveModules(driver, world, world.owner);

    await test.step("progress markers show percent, check and alert", async () => {
      expect(await driver.modulesListRowProgress("Row Percent")).toEqual({ kind: "percent", percent: 25 });
      expect(await driver.modulesListRowProgress("Row Check")).toEqual({ kind: "check" });
      expect(await driver.modulesListRowProgress("Row Alert")).toEqual({ kind: "alert" });
    });

    await test.step("names link to their detail pages", async () => {
      expect(await driver.modulesListRowDetailHref("Row Percent")).toContain(`/modules/${percentId}`);
      expect(await driver.modulesListRowDetailHref("Row Check")).toContain(`/modules/${checkId}`);
      expect(await driver.modulesListRowDetailHref("Row Alert")).toContain(`/modules/${alertId}`);
    });

    await test.step("info controls open and close the peek panel", async () => {
      expect(await driver.modulesListPeekName()).toBe(null);
      await driver.modulesListRowOpenPeek("Row Percent");
      expect(await driver.modulesListPeekName()).toBe("Row Percent");
      expect(await driver.currentPath()).toContain("peekModule=");
      await driver.modulesListPeekClose();
      expect(await driver.modulesListPeekName()).toBe(null);
      expect(await driver.currentPath()).not.toContain("peekModule=");
    });

    await test.step("small screens replace hover actions with a row menu", async () => {
      expect(await driver.modulesListRowMenuVisible("Row Percent")).toBe(false);
      await driver.setViewportSize(390, 844);
      expect(await driver.modulesListRowMenuVisible("Row Percent")).toBe(true);
    });
  }
);

test(
  specTitle(ROW_012, "clicking an archived row opens its peek panel without navigating"),
  { tag: specTags(ROW_012) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod012a");
    const id = await serverCreateModule(world.workspaceSlug, world.projectId, "Row Archived", world.owner.cookie);
    await archivesPatchModule(world.workspaceSlug, world.projectId, id, { status: "completed" }, world.owner.cookie);
    await archivesArchiveModule(world.workspaceSlug, world.projectId, id, world.owner.cookie);
    // Archived-tab entry reuses the merged archives helpers read-only;
    // this scenario proves row click routing only (MOD-012), while the
    // archived screen itself belongs to NEWFRONT-309 (MOD-027).
    await driver.openAuthenticated(
      `/${world.workspaceSlug}/projects/${world.projectId}/modules`,
      browserSessionCookies(world.owner)
    );
    await openArchivesModulesTab(driver, world.workspaceSlug, world.projectId);
    await driver.archivesAwaitModuleRow("Row Archived");
    await driver.modulesListArchivedRowClickOpensPeek("Row Archived");
    expect(await driver.currentPath()).toContain("/archives/modules");
  }
);

test(
  specTitle(ROW_020, "board cards show identity, progress, counts and actions"),
  { tag: specTags(ROW_020) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod020");
    const longName =
      "Card With An Exceptionally Long Name That Surely Truncates Inside The Narrow Board Card Column Layout";
    const cardId = await serverCreateModule(world.workspaceSlug, world.projectId, longName, world.owner.cookie);
    await seedModuleIssues(world, cardId, 2, 1);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      cardId,
      {
        status: "in-progress",
        lead_id: world.owner.userId,
        start_date: "2026-11-01",
        target_date: "2026-11-30",
      },
      world.owner.cookie
    );
    await serverCreateModule(world.workspaceSlug, world.projectId, "Card Plain", world.owner.cookie);
    await enterLiveModules(driver, world, world.owner);
    await driver.modulesListSwitchLayout("board");

    await test.step("name truncates with a full-name tooltip", async () => {
      expect(await driver.modulesListCardNameTruncated(longName)).toBe(true);
      expect(await driver.modulesListCardNameTooltip(longName)).toBe(longName);
    });

    await test.step("status, peek trigger, count, lead and progress render", async () => {
      expect(await driver.modulesListCardStatusText(longName)).toBe("In Progress");
      expect(await driver.modulesListPeekName()).toBe(null);
      await driver.modulesListCardOpenPeek(longName);
      expect(await driver.modulesListPeekName()).toBe(longName);
      await driver.modulesListPeekClose();
      expect(await driver.modulesListCardIssueCount(longName)).toContain("1/2");
      expect(await driver.modulesListCardLeadKind(longName)).toBe("avatar");
      expect(await driver.modulesListCardLeadKind("Card Plain")).toBe("none");
      const segments = await driver.modulesListCardProgressSegments(longName);
      expect(segments.widths).toHaveLength(2);
      expect(new Set(segments.colors).size).toBe(2);
      expect(await driver.modulesListCardProgressSegments("Card Plain")).toEqual({ widths: [], colors: [] });
    });

    await test.step("dates, favorite and quick menu render", async () => {
      expect(await driver.modulesListCardDatesVisible(longName)).toBe(true);
      expect(await driver.modulesListCardFavoriteVisible(longName)).toBe(true);
      expect(await driver.modulesListCardQuickMenuVisible(longName)).toBe(true);
    });
  }
);

test(
  specTitle(ROW_020, "card body navigates while card controls do not"),
  { tag: specTags(ROW_020) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod020r");
    const id = await serverCreateModule(world.workspaceSlug, world.projectId, "Card Route", world.owner.cookie);
    await enterLiveModules(driver, world, world.owner);
    await driver.modulesListSwitchLayout("board");

    await test.step("clicking the body opens the detail page", async () => {
      await driver.modulesListCardClickBody("Card Route");
      expect(await driver.currentPath()).toContain(`/modules/${id}`);
      await driver.modulesListOpenList(world.workspaceSlug, world.projectId);
      expect(await driver.modulesListBoardCardNames()).toEqual(["Card Route"]);
    });

    for (const control of ["status", "dates", "menu", "favorite"] as const) {
      await test.step(`${control} control keeps the list path`, async () => {
        await driver.modulesListCardClickControl("Card Route", control);
      });
    }

    await test.step("peek control opens the panel instead of navigating", async () => {
      await driver.modulesListCardOpenPeek("Card Route");
      const path = await driver.currentPath();
      expect(path).toContain("peekModule=");
      // The app addresses the list with a trailing slash; only the route
      // below the query string matters here.
      expect((path.split("?")[0] ?? "").replace(/\/$/, "")).toBe(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules`
      );
    });
  }
);
