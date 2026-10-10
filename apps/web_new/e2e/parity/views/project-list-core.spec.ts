// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): the project views list core — the list
// with its breadcrumb trail and tab title, the loading skeleton, the
// zero-views and no-match empty states, and row identity. Rows: VIEW-001,
// VIEW-008, VIEW-009, VIEW-010, VIEW-011. Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  browserSessionCookies,
  createProjectViewFull,
  serverPlantViewFlags,
  serverProjectViewDetail,
  serverSavedViews,
  VIEW_ACCESS,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-001"], "views list shows saved views with breadcrumb trail and tab title"),
  { tag: specTags(["VIEW-001"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v1");
    const { owner, workspaceSlug, projectId, projectName } = harness;
    const first = `V1 Alpha ${harness.tag}`;
    const second = `V1 Beta ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: first });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: second });

    await test.step("open the views list", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([first, second]));
    });

    await test.step("breadcrumb trails the project with a views crumb", async () => {
      const crumbs = await driver.viewsListBreadcrumb();
      expect(crumbs[crumbs.length - 1]).toBe("Views");
      expect(crumbs.join(" ")).toContain(projectName);
    });

    await test.step("tab title carries the project name followed by views", async () => {
      expect(await driver.viewsListTabTitle()).toBe(`${projectName} - Views`);
    });

    await test.step("header offers the Add-view action", async () => {
      expect(await driver.viewsHeaderAddVisible()).toBe(true);
    });

    await test.step("the server agrees with the screen", async () => {
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server.map((view) => view.name)).toEqual(expect.arrayContaining([first, second]));
      const visible = await driver.viewsListNames();
      for (const name of [first, second]) expect(visible).toContain(name);
    });
  }
);

test(
  specTitle(["VIEW-008"], "skeleton shows while loading, never alongside rows or an empty state"),
  { tag: specTags(["VIEW-008"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v8");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `V8 Row ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await test.step("a slowed load shows the skeleton with no rows yet", async () => {
      await driver.viewsDelayListLoad(4000);
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListSkeletonVisible(), POLL).toBe(true);
      expect(await driver.viewsListNames()).toEqual([]);
      expect(await driver.viewsEmptyTitle()).toBe("");
      expect(await driver.viewsNoMatchTitle()).toBe("");
    });

    await test.step("rows replace the skeleton once the fetch lands", async () => {
      await expect.poll(() => driver.viewsListNames(), POLL).toContain(name);
      expect(await driver.viewsListSkeletonVisible()).toBe(false);
      expect(await driver.viewsEmptyTitle()).toBe("");
    });
  }
);

test(
  specTitle(["VIEW-009"], "zero-views empty state with a creation shortcut for every role"),
  { tag: specTags(["VIEW-009"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v9");

    await test.step("owner sees the onboarding empty state with an enabled action", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
      expect(await driver.viewsEmptyCreateVisible()).toBe(true);
      expect(await driver.viewsEmptyCreateEnabled()).toBe(true);
    });

    await test.step("the action opens the create dialog", async () => {
      await driver.viewsEmptyCreateOpen();
      await expect.poll(() => driver.modalTextContains("Create View"), POLL).toBe(true);
    });

    await test.step("a guest sees the same state with the action still enabled", async () => {
      const guest = await viewsSeat(harness, ROLE.GUEST, "parity-v9-guest");
      await driver.openAuthenticated(
        `/${harness.workspaceSlug}/projects/${harness.projectId}/views`,
        browserSessionCookies(guest)
      );
      await expect.poll(() => driver.viewsEmptyTitle(), POLL).toBe("Save custom views for your project");
      expect(await driver.viewsEmptyCreateVisible()).toBe(true);
      expect(await driver.viewsEmptyCreateEnabled()).toBe(true);
      await driver.viewsEmptyCreateOpen();
      await expect.poll(() => driver.modalTextContains("Create View"), POLL).toBe(true);
    });
  }
);

test(
  specTitle(["VIEW-010"], "no-match empty state when search excludes every view"),
  { tag: specTags(["VIEW-010"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v10");
    const { owner, workspaceSlug, projectId } = harness;
    const first = `V10 Alpha ${harness.tag}`;
    const second = `V10 Beta ${harness.tag}`;
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: first });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: second });

    await test.step("a nonsense search shows the no-match state with no rows", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([first, second]));
      await driver.viewsSearchOpen();
      await driver.viewsSearchType("zzz-no-such-view-zzz");
      await expect.poll(() => driver.viewsNoMatchTitle(), POLL).toBe("No matching results.");
      expect(await driver.viewsListNames()).toEqual([]);
      expect(await driver.viewsEmptyTitle()).toBe("");
    });

    await test.step("clearing restores the untouched views", async () => {
      await driver.viewsSearchClear();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([first, second]));
      expect(await driver.viewsNoMatchTitle()).toBe("");
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server.map((view) => view.name)).toEqual(expect.arrayContaining([first, second]));
    });
  }
);

test(
  specTitle(["VIEW-011"], "row identity: icon, name, deep link, access badge, owner, Live state"),
  { tag: specTags(["VIEW-011"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-v11");
    const { owner, workspaceSlug, projectId } = harness;
    const plain = `V11 Plain ${harness.tag}`;
    const glyph = `V11 Glyph ${harness.tag}`;
    const marked = "🚀";
    const plainId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: plain });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, {
      name: glyph,
      // Emoji logos store decimal codepoints (U+1F680), rendered as glyphs.
      logo_props: { in_use: "emoji", emoji: { value: "128640" } },
    });

    await test.step("rows show names with deep links to their detail pages", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([plain, glyph]));
      expect(await driver.viewsRowHref(plain)).toBe(`/${workspaceSlug}/projects/${projectId}/views/${plainId}`);
      expect(await driver.viewsRowHref("no such view")).toBeNull();
    });

    await test.step("the custom icon renders on its row only", async () => {
      expect(await driver.viewsRowText(glyph)).toContain(marked);
      expect(await driver.viewsRowText(plain)).not.toContain(marked);
    });

    await test.step("fresh views badge Public with the owner's avatar and no Live marker", async () => {
      expect(await driver.viewsRowAccess(plain)).toBe("Public");
      expect(await driver.viewsRowOwnerAvatar(plain)).toBe(true);
      expect(await driver.viewsRowLiveVisible(plain)).toBe(false);
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, plainId, owner.cookie);
      expect(detail.access).toBe(VIEW_ACCESS.PUBLIC);
      expect(detail.owned_by).toBe(owner.userId);
    });

    await test.step("an owner-only view badges Private", async () => {
      await serverPlantViewFlags(plainId, { access: VIEW_ACCESS.PRIVATE });
      await driver.reloadPage();
      await expect.poll(() => driver.viewsListNames(), POLL).toContain(plain);
      await expect.poll(() => driver.viewsRowAccess(plain), POLL).toBe("Private");
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, plainId, owner.cookie);
      expect(detail.access).toBe(VIEW_ACCESS.PRIVATE);
    });
  }
);
