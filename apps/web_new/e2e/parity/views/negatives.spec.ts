// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the negative rows — no live updates
// (VIEW-043), keyboard support (VIEW-044), shareable deep links (VIEW-045),
// desktop parity (VIEW-046) and no drag-and-drop/exports (VIEW-047).
// Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  browserSessionCookies,
  createProjectViewFull,
  createWorkspaceViewFull,
  patchProjectView,
  patchWorkspaceView,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import {
  viewsHarness,
  viewsOpenDetailAs,
  viewsOpenListAs,
  viewsSeat,
  wsViewsOpenDetailAs,
  wsViewsOpenListAs,
} from "./support";

const POLL = { timeout: 60_000 };

test.describe("views negative rows", () => {
  test(
    specTitle(["VIEW-043"], "other users' changes never refresh lists or details without a reload"),
    { tag: specTags(["VIEW-043"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vlive");
      const { workspaceSlug, projectId, owner, projectName } = harness;
      const member = await viewsSeat(harness, ROLE.MEMBER, "parity-vlive-member");
      const first = `Vlive First ${harness.tag}`;
      const firstId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: first });
      const wsFirst = `Vlive Ws First ${harness.tag}`;
      const wsFirstId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: wsFirst });

      // The member watches (fresh views default to Public); the owner
      // mutates, since view PATCH is owner-only server-side.
      await test.step("the project list stays stale until reloaded", async () => {
        await viewsOpenListAs(driver, harness, member);
        await expect.poll(() => driver.viewsListNames(), POLL).toEqual([first]);
        const second = `Vlive Second ${harness.tag}`;
        await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: second });
        expect(await driver.viewsListNames()).toEqual([first]);
        await driver.reloadPage();
        // The default sort lists the newest view first.
        await expect.poll(() => driver.viewsListNames(), POLL).toEqual([second, first]);
      });

      await test.step("the project detail stays stale until reloaded", async () => {
        await viewsOpenDetailAs(driver, harness, firstId, member);
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${first}`);
        const renamed = `Vlive Renamed ${harness.tag}`;
        await patchProjectView(workspaceSlug, projectId, firstId, owner.cookie, { name: renamed });
        expect(await driver.viewsDetailTabTitle()).toBe(`${projectName} - ${first}`);
        await driver.reloadPage();
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${renamed}`);
      });

      await test.step("the workspace list and detail stay stale until reloaded", async () => {
        await wsViewsOpenListAs(driver, harness, member);
        await expect.poll(() => driver.wsViewsListNames(), POLL).toContain(wsFirst);
        const wsSecond = `Vlive Ws Second ${harness.tag}`;
        await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: wsSecond });
        expect(await driver.wsViewsListNames()).not.toContain(wsSecond);
        await wsViewsOpenDetailAs(driver, harness, wsFirstId, member);
        await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", wsFirst]);
        const wsRenamed = `Vlive Ws Renamed ${harness.tag}`;
        await patchWorkspaceView(workspaceSlug, wsFirstId, owner.cookie, { name: wsRenamed });
        expect(await driver.wsViewsDetailCrumbs()).toEqual(["Views", wsFirst]);
        await driver.reloadPage();
        await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", wsRenamed]);
      });

      // The acting user's own mutations update immediately instead; that
      // direction is covered by the row-actions and detail-filters specs.
    }
  );

  test(
    specTitle(["VIEW-044"], "Escape closes the create dialogs and dialogs tab in field order"),
    { tag: specTags(["VIEW-044"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vkey");

      await test.step("Escape closes the project create dialog", async () => {
        await viewsOpenListAs(driver, harness);
        await driver.viewsOpenCreateFromHeader();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        await driver.viewsDialogEscape();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      });

      await test.step("Escape closes the workspace create dialog", async () => {
        await wsViewsOpenDetailAs(driver, harness, "all-issues");
        await driver.wsViewsDetailAddClick();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        await driver.viewsDialogEscape();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      });

      await test.step("bug: NEWFRONT-241 the dialog tabs title, actions, wrap — skipping Description", async () => {
        // bug: NEWFRONT-241 — the Description field between Title and the
        // actions never receives focus; the intended order is Title,
        // Description, then the actions.
        await viewsOpenListAs(driver, harness);
        await driver.viewsOpenCreateFromHeader();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        const order = await driver.viewsDialogTabOrder();
        const joined = order.join("|");
        expect(joined).toContain("Title|Cancel|Create View");
        expect(joined).not.toContain("Description");
        await driver.viewsDialogEscape();
      });

      // Escape in the list search (clear, then collapse) is covered by
      // VIEW-004. No other view shortcuts exist: the only key handler in
      // the views components is the search field's Escape handler
      // (`view-list-header.tsx` handleInputKeyDown), a modifier-key grep
      // over both views component trees finds nothing, and dialogs close
      // through the shared modal library's Escape handling.
    }
  );

  test(
    specTitle(["VIEW-045"], "copied links reopen the same project and workspace detail state"),
    { tag: specTags(["VIEW-045"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vlink");
      const { workspaceSlug, projectId, owner, projectName } = harness;
      const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vlink-guest");
      const project = `Vlink Project ${harness.tag}`;
      const projectId2 = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: project });
      const custom = `Vlink Space ${harness.tag}`;
      const customId = await createWorkspaceViewFull(workspaceSlug, owner.cookie, { name: custom });

      await test.step("a copied project link loads the same detail", async () => {
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsListNames(), POLL).toContain(project);
        const copied = await driver.viewsRowCopyLink(project);
        expect(copied).toContain(`/projects/${projectId}/views/${projectId2}`);
        await driver.openAuthenticated(new URL(copied).pathname, browserSessionCookies(owner));
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${project}`);
        await expect.poll(() => driver.viewsDetailBreadcrumb(), POLL).toEqual([projectName, "Views", project]);
      });

      await test.step("a copied workspace link loads the same detail", async () => {
        // The workspace tab title stays "All Views" even for customs; the
        // crumbs carry the current view's name.
        await wsViewsOpenDetailAs(driver, harness, customId);
        await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", custom]);
        await driver.wsViewsDetailMenuOpen();
        const copied = await driver.wsViewsDetailCopyLink();
        expect(copied).toContain(`/workspace-views/${customId}`);
        await driver.openAuthenticated(new URL(copied).pathname, browserSessionCookies(owner));
        await expect.poll(() => driver.wsViewsDetailCrumbs(), POLL).toEqual(["Views", custom]);
      });

      await test.step("a user without access gets the missing-view state instead", async () => {
        await viewsOpenDetailAs(driver, harness, projectId2, guest);
        await expect.poll(() => driver.viewsDetailErrorTitle(), POLL).toBe("View does not exist");
      });
    }
  );

  test(
    specTitle(["VIEW-046"], "views pages call no desktop runtime and render the same shell"),
    { tag: specTags(["VIEW-046"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vdesk");
      const { workspaceSlug, projectId, owner, projectName } = harness;
      const seen = `Vdesk Listed ${harness.tag}`;
      const seenId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: seen });

      await test.step("no native bridge is exposed to the page", async () => {
        await viewsOpenListAs(driver, harness);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });

      await driver.desktopRuntimeStartRequestSpy();
      await test.step("browsing list, detail and switcher calls no runtime endpoint", async () => {
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsListNames(), POLL).toContain(seen);
        await viewsOpenDetailAs(driver, harness, seenId);
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${seen}`);
        await driver.viewsDetailSwitcherOpen(seen);
        expect(await driver.viewsDetailSwitcherOptions()).toContain(seen);
        expect(await driver.desktopRuntimeSpyUrls()).toEqual([]);
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });
    }
  );

  test(
    specTitle(["VIEW-047"], "rows cannot be reordered by drag and no export action exists"),
    { tag: specTags(["VIEW-047"]) },
    async ({ driver }) => {
      const harness = await viewsHarness("parity-vdrag");
      const { workspaceSlug, projectId, owner, projectName } = harness;
      const alpha = `Vdrag Alpha ${harness.tag}`;
      const beta = `Vdrag Beta ${harness.tag}`;
      const alphaId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: alpha });
      await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: beta });

      await test.step("dragging a row leaves the sort order untouched", async () => {
        await viewsOpenListAs(driver, harness);
        await expect.poll(() => driver.viewsListNames(), POLL).toEqual([beta, alpha]);
        await driver.viewsListDragRow(alpha, beta);
        expect(await driver.viewsListNames()).toEqual([beta, alpha]);
        await driver.reloadPage();
        await expect.poll(() => driver.viewsListNames(), POLL).toEqual([beta, alpha]);
      });

      await test.step("no export, import or print action exists on list, detail or dialog", async () => {
        expect(await driver.viewsPageExportImportVisible()).toBe(false);
        await viewsOpenDetailAs(driver, harness, alphaId);
        await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toBe(`${projectName} - ${alpha}`);
        expect(await driver.viewsPageExportImportVisible()).toBe(false);
        await viewsOpenListAs(driver, harness);
        await driver.viewsOpenCreateFromHeader();
        await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
        expect(await driver.viewsPageExportImportVisible()).toBe(false);
        await driver.viewsDialogEscape();
      });
    }
  );
});
