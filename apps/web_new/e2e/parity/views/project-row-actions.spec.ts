// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): project row actions — starring,
// the permission-gated quick menu, copy-link/new-tab, edit and delete.
// Rows: VIEW-012, VIEW-015, VIEW-016, VIEW-017, VIEW-018, VIEW-020 (OSS
// part). Green on apps/web first.
import { test, expect } from "../fixtures";
import {
  ROLE,
  createProjectViewFull,
  serverProjectViewDetail,
  serverSavedViews,
  serverUserFavorites,
  setProjectViewFlags,
} from "../helpers/api";
import { isCloudNewEdition } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenListAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

test(
  specTitle(["VIEW-012"], "star toggles the server favorite silently; guests see no star"),
  { tag: specTags(["VIEW-012"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vf12");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vf12 Star ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await test.step("starring writes a view favorite with no notice", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      expect(await driver.viewsRowStarVisible(name)).toBe(true);
      await driver.viewsToggleStar(name);
      await expect.poll(() => driver.viewsRowStarSelected(name), POLL).toBe(true);
      await expect
        .poll(
          async () =>
            (await serverUserFavorites(workspaceSlug, owner.cookie)).filter(
              (f) => f.entity_type === "view" && f.entity_identifier === created
            ).length,
          POLL
        )
        .toBe(1);
      expect(await driver.lastToast()).toBeNull();
    });

    await test.step("unstarring removes it, still silently", async () => {
      await driver.viewsToggleStar(name);
      await expect.poll(() => driver.viewsRowStarSelected(name), POLL).toBe(false);
      await expect
        .poll(
          async () =>
            (await serverUserFavorites(workspaceSlug, owner.cookie)).filter(
              (f) => f.entity_type === "view" && f.entity_identifier === created
            ).length,
          POLL
        )
        .toBe(0);
      expect(await driver.lastToast()).toBeNull();
    });

    await test.step("guests see rows but no star", async () => {
      const guest = await viewsSeat(harness, ROLE.GUEST, "parity-vf12-guest");
      await setProjectViewFlags(workspaceSlug, projectId, owner.cookie, { guest_view_all_features: true });
      await viewsOpenListAs(driver, harness, guest);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      expect(await driver.viewsRowStarVisible(name)).toBe(false);
    });
  }
);

test(
  specTitle(["VIEW-017", "VIEW-018", "VIEW-020"], "row menu entries follow ownership; copy and new-tab work"),
  { tag: specTags(["VIEW-017", "VIEW-018", "VIEW-020"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vm17");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vm17 Menu ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-vm17-member");
    const admin = await viewsSeat(harness, ROLE.ADMIN, "parity-vm17-admin");

    await test.step("owners see edit and delete, others see subsets, publish is absent", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      // Publish's absence is the OSS half of VIEW-020; a cloud build serves
      // the entry for the owner and every admin, and its own leg below
      // covers the publish flow.
      const cloud = driver.target === "web_new" && isCloudNewEdition();
      const ownerItems = cloud
        ? ["Edit", "Open in new tab", "Copy link", "Publish", "Delete"]
        : ["Edit", "Open in new tab", "Copy link", "Delete"];
      await driver.viewsRowMenuOpen(name);
      expect(await driver.viewsRowMenuItems()).toEqual(ownerItems);
      if (!cloud) expect(await driver.viewsRowMenuPublishPresent()).toBe(false);

      await viewsOpenListAs(driver, harness, member);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      await driver.viewsRowMenuOpen(name);
      expect(await driver.viewsRowMenuItems()).toEqual(["Open in new tab", "Copy link"]);

      await viewsOpenListAs(driver, harness, admin);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      await driver.viewsRowMenuOpen(name);
      expect(await driver.viewsRowMenuItems()).toEqual(
        cloud ? ["Open in new tab", "Copy link", "Publish", "Delete"] : ["Open in new tab", "Copy link", "Delete"]
      );
    });

    await test.step("copy link writes the deep link and toasts", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      const clip = await driver.viewsRowCopyLink(name);
      await expect.poll(() => driver.lastToast(), POLL).toContain("Link Copied");
      expect(clip).toContain(`/projects/${projectId}/views/${created}`);
    });

    await test.step("open in new tab loads the same view standalone", async () => {
      const href = await driver.viewsRowOpenNewTabHref(name);
      expect(href).toContain(`/projects/${projectId}/views/${created}`);
    });
  }
);

test(
  specTitle(["VIEW-015"], "edit dialog prefills and saves silently; failure toasts and restores originals"),
  { tag: specTags(["VIEW-015"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-ve15");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Ve15 Edit ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, {
      name,
      description: "before",
    });

    await test.step("the dialog opens prefilled for the owner", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      await driver.viewsRowMenuOpen(name);
      await driver.viewsRowMenuPick("Edit");
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      expect(await driver.viewsDialogHeading()).toBe("Update View");
      expect(await driver.viewsDialogTitleValue()).toBe(name);
      expect(await driver.viewsDialogDescriptionValue()).toBe("before");
    });

    await test.step("saving persists without a notice", async () => {
      const renamed = `${name} v2`;
      await driver.viewsDialogFillTitle(renamed);
      await driver.viewsDialogFillDescription("after");
      await driver.viewsDialogSubmit();
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(false);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([renamed]);
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, created, owner.cookie);
      expect(detail.name).toBe(renamed);
      expect(detail.description).toBe("after");
      expect(await driver.lastToast()).toBeNull();
    });

    // The shared form resets on failure (NEWFRONT-221); for edits the
    // reset restores the prefilled originals, so nothing saved is lost.
    await test.step("a failing update toasts and restores the originals", async () => {
      const latest = `${name} v2`;
      await driver.viewsRowMenuOpen(latest);
      await driver.viewsRowMenuPick("Edit");
      await expect.poll(() => driver.viewsDialogOpen(), POLL).toBe(true);
      await driver.viewsDialogFillTitle(`${latest} v3`);
      await driver.viewsFailNextWrite(500);
      await driver.viewsDialogSubmitAttempt();
      await expect.poll(() => driver.lastToast(), POLL).toContain("Failed to update view");
      expect(await driver.viewsDialogOpen()).toBe(true);
      await expect.poll(() => driver.viewsDialogTitleValue(), POLL).toBe(latest);
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, created, owner.cookie);
      expect(detail.name).toBe(latest);
    });
  }
);

test(
  specTitle(["VIEW-016"], "delete confirms explicitly, then removes and returns to the list"),
  { tag: specTags(["VIEW-016"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd16");
    const { owner, workspaceSlug, projectId } = harness;
    const doomed = `Vd16 Doomed ${harness.tag}`;
    const spared = `Vd16 Spared ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: doomed });
    await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: spared });

    await test.step("the confirmation warns about permanent loss; cancel keeps the view", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([doomed, spared]));
      await driver.viewsRowMenuOpen(doomed);
      await driver.viewsRowMenuPick("Delete");
      await expect.poll(() => driver.viewsDeleteTitle(), POLL).toBe("Are you sure you want to delete this view?");
      const body = await driver.viewsDeleteBody();
      expect(body).toContain("permanently deleted");
      expect(body).toContain("without any way to restore them");
      await driver.viewsDeleteCancel();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual(expect.arrayContaining([doomed, spared]));
    });

    await test.step("confirming deletes, toasts, and stays on the list", async () => {
      await driver.viewsRowMenuOpen(doomed);
      await driver.viewsRowMenuPick("Delete");
      await driver.viewsDeleteConfirm();
      await expect.poll(() => driver.lastToast(), POLL).toContain("View deleted successfully");
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([spared]);
      expect(driver.page.url()).toContain("/views");
      expect(driver.page.url()).not.toContain(created);
      // NOTE: delete is a soft-delete for the list; the retrieve endpoint
      // still answers 200 for the deleted row, so list exclusion (not a
      // 404) is the server proof of removal.
      const server = await serverSavedViews(workspaceSlug, projectId, owner.cookie);
      expect(server.map((v) => v.name)).toEqual([spared]);
    });

    await test.step("a failing delete toasts and keeps the view", async () => {
      await driver.viewsRowMenuOpen(spared);
      await driver.viewsRowMenuPick("Delete");
      await driver.viewsFailNextWrite(500);
      await driver.viewsDeleteConfirmAttempt();
      await expect.poll(() => driver.lastToast(), POLL).toContain("View could not be deleted");
      expect(await driver.viewsDeleteTitle()).toBe("Are you sure you want to delete this view?");
      await driver.viewsDeleteCancel();
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([spared]);
    });
  }
);

test(
  specTitle(["VIEW-020"], "cloud: publishing shows Live and a link, unpublishing clears both"),
  { tag: specTags(["VIEW-020"]) },
  async ({ driver }, testInfo) => {
    // Cloud leg (parity/specs/views.md §6/§14, gate note): target-only. The
    // old cloud app's publish modules are empty stubs, so there is no oracle
    // half; this proves web_new against the cloud build, which the run
    // declares with PARITY_NEW_EDITION=cloud.
    test.skip(testInfo.project.name !== "new", "the publish flow pins the web_new cloud build only");
    test.skip(!isCloudNewEdition(), "cloud leg needs web_new built with the cloud edition");
    const harness = await viewsHarness("parity-vm20c");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vm20c Live ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });

    await test.step("the owner's menu offers publish, and publishing stores an anchor", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      await driver.viewsRowMenuOpen(name);
      expect(await driver.viewsRowMenuPublishPresent()).toBe(true);
      await driver.page.keyboard.press("Escape");
      const link = await driver.viewsRowPublish(name);
      expect(link).toContain(`/views/published/`);
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, created, owner.cookie);
      expect(detail.anchor).not.toBeNull();
      expect(link).toContain(encodeURIComponent(detail.anchor ?? ""));
    });

    await test.step("the row shows Live while published", async () => {
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      expect(await driver.viewsRowLiveVisible(name)).toBe(true);
    });

    await test.step("unpublishing clears the anchor and the Live marker", async () => {
      await driver.viewsRowUnpublish(name);
      await expect
        .poll(async () => (await serverProjectViewDetail(workspaceSlug, projectId, created, owner.cookie)).anchor, POLL)
        .toBeNull();
      await viewsOpenListAs(driver, harness);
      await expect.poll(() => driver.viewsListNames(), POLL).toEqual([name]);
      expect(await driver.viewsRowLiveVisible(name)).toBe(false);
      await driver.viewsRowMenuOpen(name);
      expect(await driver.viewsRowMenuPublishPresent()).toBe(true);
    });
  }
);
