// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-125): sidebar rows are permission-gated,
// highlight from the URL, and hide unless pinned or built in.
// Observed on the running old app: the workspace sidebar renders member
// rows (home, personal, drafts, aggregate views, projects) with the
// current-URL row in a distinct tone; with More closed the main sidebar
// holds exactly the built-in rows while unpinned extra destinations stay
// hidden; pin writes persist through the sidebar-preferences endpoint;
// guests see the shared rows but no member-only rows (drafts, analytics,
// archives), no Favorites section and no project-creation affordance.
// Row: SHELL-046.
import { test, expect } from "../../fixtures";
import {
  WORKSPACE_ROLE_GUEST,
  deleteWorkspace,
  ensureWorkspace,
  ensureWorkspaceMember,
  getSidebarPreferences,
  patchSidebarPreferences,
  ownerSession,
  patchUserProperties,
  workspaceMemberRoles,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-046"];
const GUEST_EMAIL = "parity-sidebar-guest@example.com";
const GUEST_PASSWORD = "Parity-Guest-1";

test(
  specTitle(ROWS, "sidebar rows gate by role, highlight the URL and follow pins"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // One server session for the whole scenario keeps the scratch API far
    // below its rate limit even while sibling runs share the stack.
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    // Uncapped baseline: an interrupted overflow run may have left a cap.
    await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("member rows render on home", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect
        .poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(["Home", "Drafts", "Work Items", seed.projectName]));
    });

    await test.step("the current-URL row reads active", async () => {
      const home = await driver.sidebarRowTone("Home");
      const drafts = await driver.sidebarRowTone("Drafts");
      expect(home.background !== drafts.background || home.color !== drafts.color).toBe(true);
    });

    await test.step("the highlight follows navigation", async () => {
      await driver.openWorkspacePath(`/${seed.workspaceSlug}/drafts/`);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Drafts");
      const home = await driver.sidebarRowTone("Home");
      const drafts = await driver.sidebarRowTone("Drafts");
      expect(home.background !== drafts.background || home.color !== drafts.color).toBe(true);
      const homeActive = home.background !== "rgba(0, 0, 0, 0)";
      const draftsActive = drafts.background !== "rgba(0, 0, 0, 0)";
      expect(draftsActive && !homeActive).toBe(true);
    });

    await test.step("unpinned non-built-in rows stay hidden", async () => {
      // The relocated layout renders extra destinations inside the More
      // disclosure instead of the main sidebar, so with More closed the main
      // sidebar holds exactly the built-in rows. The loaded marker comes
      // first: a bare absence check would pass vacuously on the pre-load
      // empty list.
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setMoreSectionOpen(false);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      const links = await driver.sidebarLinkTexts();
      expect(links).toEqual(expect.arrayContaining(["Home", "Drafts", "Work Items", seed.projectName]));
      for (const absent of ["Analytics", "Cycles", "Archives", "Prompts", "Schedulers"]) {
        expect(links).not.toContain(absent);
      }
    });

    await test.step("pin writes persist on the server", async () => {
      // A dedicated workspace keeps the check to a single member's rows: the
      // preferences write endpoint scopes by key and workspace, not by user.
      const pins = await ensureWorkspace(session, "Parity Pins", "parity-pins");
      const before = await getSidebarPreferences(pins.slug, session);
      await patchSidebarPreferences(pins.slug, session, [
        { key: "active_cycles", is_pinned: true, sort_order: before["active_cycles"]?.sort_order ?? 0 },
      ]);
      const server = await getSidebarPreferences(pins.slug, session);
      expect(server["active_cycles"]?.is_pinned).toBe(true);
      await patchSidebarPreferences(pins.slug, session, [
        {
          key: "active_cycles",
          is_pinned: before["active_cycles"]?.is_pinned ?? false,
          sort_order: before["active_cycles"]?.sort_order ?? 0,
        },
      ]);
      const restored = await getSidebarPreferences(pins.slug, session);
      expect(restored["active_cycles"]?.is_pinned).toBe(before["active_cycles"]?.is_pinned ?? false);
      await deleteWorkspace(session, pins.slug);
    });

    await test.step("guests never see member-only rows", async () => {
      await ensureWorkspaceMember(seed.workspaceSlug, session, GUEST_EMAIL, GUEST_PASSWORD, WORKSPACE_ROLE_GUEST);
      const server = await workspaceMemberRoles(seed.workspaceSlug, session);
      expect(server.find((member) => member.email === GUEST_EMAIL)?.role).toBe(WORKSPACE_ROLE_GUEST);
      await expect.poll(() => driver.isCreateProjectVisible(), { timeout: 60_000 }).toBe(true);
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(GUEST_EMAIL, GUEST_PASSWORD);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      // The loaded marker (Home) comes first so the absences below prove a
      // rendered guest sidebar, not a pre-load empty list.
      const links = await driver.sidebarLinkTexts();
      expect(links).toEqual(expect.arrayContaining(["Home", "Work Items"]));
      expect(links).not.toContain("Drafts");
      expect(await driver.sidebarSectionNames()).not.toContain("Favorites");
      // Member-gated More rows stay hidden even with the disclosure open,
      // while the shared ones render.
      await driver.setMoreSectionOpen(true);
      const more = (await driver.moreSectionLinks()).map((link) => link.text);
      expect(more).toEqual(expect.arrayContaining(["Schedulers"]));
      for (const absent of ["Analytics", "Archives"]) {
        expect(more).not.toContain(absent);
      }
      await expect.poll(() => driver.isCreateProjectVisible(), { timeout: 30_000 }).toBe(false);
    });
  }
);
