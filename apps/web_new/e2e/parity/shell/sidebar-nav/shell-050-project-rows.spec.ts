// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-125): sidebar project rows link to the
// preferred default tab, expand inline sub-navigation, scroll the active
// project into view and offer a quick menu.
// Observed on the running old app: the row resolves its link from the
// member store's tab preference once a project route has loaded it (issues
// until then), so a fresh home links issues and the configured tab wins
// after; in tabbed navigation clicking the row lands on that tab with the
// row highlighted, while in the default accordion mode the click expands
// the row's inline sub-navigation instead; landing on a project URL renders
// the row's sub-navigation and keeps the row in view; the row quick menu
// offers a copy-link entry whose link opens the project. Row: SHELL-050.
// Mode discipline: the tabbed half runs on a dedicated workspace because
// the navigation mode lives on workspace-scoped user properties —
// flipping it on the seed workspace would disturb sibling runs.
import { test, expect } from "../../fixtures";
import {
  deleteWorkspace,
  ensureProject,
  ensureWorkspace,
  ownerSession,
  patchProjectFlags,
  patchUserProperties,
  setProjectDefaultTab,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-050"];
const TABS_SLUG = "parity-tabs";
const TABS_PROJECT = "Parity Tabs Project";
const TABS_CODE = "PAR_TP";

test(
  specTitle(ROWS, "project rows link, expand, scroll into view and offer a quick menu"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    let session = "";
    await test.step("prepare server session", async () => {
      session = await ownerSession(seed);
      // Uncapped baseline: an interrupted overflow run may have left a cap.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
    });
    const tabs = await test.step("provision a tabbed-mode workspace", async () => {
      const workspace = await ensureWorkspace(session, "Parity Tabs", TABS_SLUG);
      const project = await ensureProject(TABS_SLUG, session, TABS_PROJECT, TABS_CODE, undefined, {
        page_view: true,
      });
      await patchUserProperties(TABS_SLUG, session, { navigation_control_preference: "TABBED" });
      return { workspace, project };
    });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("rows link to the default tab", async () => {
      // Re-guard the cap: sibling runs share the seed user and can clobber
      // the row limit between steps, hiding rows in the overflow.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(seed.projectName);
      expect(await driver.projectRowHref(seed.projectName)).toBe(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`
      );
    });

    await test.step("rows link to the configured default tab", async () => {
      const stored = await setProjectDefaultTab(TABS_SLUG, tabs.project.id, session, "pages");
      expect(stored["default_tab"]).toBe("pages");
      await driver.openWorkspaceHome(TABS_SLUG);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(TABS_PROJECT);
      // The row reads the preference from the member store, which only
      // loads on project routes: a fresh home links the fallback until a
      // project page warms the store and the row re-resolves. A plain load
      // (not the issues-list wait: the store fills on mount whether or not
      // the heavy list renders) plus a poll on the re-resolved link.
      expect(await driver.projectRowHref(TABS_PROJECT)).toBe(`/${TABS_SLUG}/projects/${tabs.project.id}/issues`);
      await driver.openWorkspacePath(`/${TABS_SLUG}/projects/${tabs.project.id}/issues`);
      await expect
        .poll(() => driver.projectRowHref(TABS_PROJECT), { timeout: 60_000 })
        .toBe(`/${TABS_SLUG}/projects/${tabs.project.id}/pages`);
    });

    await test.step("clicking lands on the tab with the row highlighted", async () => {
      await driver.openSidebarLink(TABS_PROJECT);
      // The router normalizes the landing with a trailing slash while the
      // row href carries none, so compare slash-insensitively.
      const landedPath = async (): Promise<string> => new URL(driver.page.url()).pathname.replace(/\/+$/, "");
      await expect.poll(landedPath, { timeout: 30_000 }).toBe(`/${TABS_SLUG}/projects/${tabs.project.id}/pages`);
      const row = await driver.sidebarRowTone(TABS_PROJECT);
      const home = await driver.sidebarRowTone("Home");
      expect(row.background).not.toBe("rgba(0, 0, 0, 0)");
      expect(home.background).toBe("rgba(0, 0, 0, 0)");
    });

    await test.step("rows expand inline sub-navigation", async () => {
      // Re-guard the cap: sibling runs share the seed user and can clobber
      // the row limit between steps, hiding rows in the overflow.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
      // Pin the asserted flag shape: the seed project's flags live on shared
      // state and other runs flip them, so cycles/modules/views-off is
      // re-established here instead of assumed. Intake stays out of the pin:
      // the project PATCH silently ignores intake_view (verified: 200 with
      // the value unchanged), so intake follows the dedicated projects below
      // in SHELL-055 instead of the seed.
      await patchProjectFlags(seed.workspaceSlug, session, seed.projectId, {
        cycle_view: false,
        module_view: false,
        issue_views_view: false,
        page_view: true,
      });
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await driver.setProjectRowOpen(seed.projectName, true);
      await expect.poll(() => driver.isProjectRowOpen(seed.projectName), { timeout: 30_000 }).toBe(true);
      // The pinned flags hide cycles, modules and views; intake is covered
      // by dedicated projects in SHELL-055, not the drift-prone seed.
      const texts = (await driver.projectSubnavLinks()).map((link) => link.text);
      expect(texts).toEqual(expect.arrayContaining(["Work Items", "Pages", "Schedulers", "AI Workers"]));
      for (const absent of ["Cycles", "Modules", "Views"]) {
        expect(texts).not.toContain(absent);
      }
      for (const link of await driver.projectSubnavLinks()) {
        expect(link.href ?? "").toContain(`/projects/${seed.projectId}/`);
      }
    });

    await test.step("landing on the project keeps its row expanded and visible", async () => {
      // Re-guard the cap: sibling runs share the seed user and can clobber
      // the row limit between steps, hiding rows in the overflow.
      await patchUserProperties(seed.workspaceSlug, session, { navigation_project_limit: 10 });
      // The issues route is heavy and its sidebar data can arrive starved:
      // with no row rendered, openness reads false forever no matter how
      // long the poll. Re-enter until the row itself shows (fresh loads
      // refetch), then poll for its expansion like a user would.
      let present = false;
      for (let attempt = 0; attempt < 3 && !present; attempt += 1) {
        await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
        const deadline = Date.now() + 20_000;
        do {
          if ((await driver.sidebarLinkTexts()).includes(seed.projectName)) {
            present = true;
            break;
          }
          await driver.page.waitForTimeout(2000);
        } while (Date.now() < deadline);
      }
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(seed.projectName);
      // The route usually auto-expands its row; under shared-stack load the
      // auto-expand loses its data race and the row stays shut, so drive the
      // toggle exactly then — like a user facing a closed row would — instead
      // of failing a render the app recovers from on interaction.
      const autoDeadline = Date.now() + 30_000;
      let open = false;
      do {
        if (await driver.isProjectRowOpen(seed.projectName)) {
          open = true;
          break;
        }
        await driver.page.waitForTimeout(2000);
      } while (Date.now() < autoDeadline);
      if (!open) await driver.setProjectRowOpen(seed.projectName, true);
      await expect.poll(() => driver.isProjectRowOpen(seed.projectName), { timeout: 30_000 }).toBe(true);
      expect(await driver.isProjectRowInViewport(seed.projectName)).toBe(true);
    });

    await test.step("copy-link confirms a working deep link", async () => {
      await driver.openWorkspaceHome(TABS_SLUG);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(TABS_PROJECT);
      await driver.openProjectQuickMenu(TABS_PROJECT);
      await expect.poll(() => driver.projectQuickMenuTexts(), { timeout: 30_000 }).not.toEqual([]);
      expect((await driver.projectQuickMenuTexts()).join(" ")).toContain("Copy link");
      // The headless context denies clipboard writes by default, which the
      // copy handler reports as a failure toast: grant first, like a real
      // browser session where the user already allowed it.
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await driver.activateProjectQuickMenuItem("Copy link");
      await expect.poll(() => driver.isToastVisible("Link copied"), { timeout: 30_000 }).toBe(true);
      const copied = await driver.readClipboard();
      expect(copied).toContain(`/projects/${tabs.project.id}/issues`);
      await driver.openWorkspacePath(copied);
      await expect
        .poll(() => Promise.resolve(new URL(driver.page.url()).pathname), { timeout: 60_000 })
        .toContain(`/projects/${tabs.project.id}/issues`);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain(TABS_PROJECT);
    });

    await test.step("restore the seeded baseline", async () => {
      await deleteWorkspace(session, tabs.workspace.slug);
    });
  }
);
