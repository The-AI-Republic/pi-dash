// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-42): project view detail header overflow
// menu — the detail half of VIEW-017 and the canonical-copy half of
// VIEW-018/045. Green on apps/web first. The header menu replaces the
// generic list-page ellipsis: edit for the owner, delete for owner or
// admin, open/copy for everyone, publish on cloud after copy. Copy
// writes the canonical `/{ws}/projects/{p}/views/{id}` even when the
// detail URL carries peek or layout params.
import { test, expect } from "../fixtures";
import { ROLE, createProjectViewFull, serverProjectViewDetail, browserSessionCookies } from "../helpers/api";
import { isCloudNewEdition } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import { viewsHarness, viewsOpenDetailAs, viewsSeat } from "./support";

const POLL = { timeout: 60_000 };

async function openDetailMenu(driver: unknown): Promise<void> {
  const d = driver as {
    page: import("playwright").Page;
  };
  const trigger = d.page.getByTestId("project-view-detail-menu-trigger");
  await trigger.waitFor({ state: "visible", timeout: 30_000 });
  await trigger.click({ timeout: 30_000 });
  await d.page.getByRole("menuitem").first().waitFor({ timeout: 30_000 });
}

async function detailMenuItems(driver: unknown): Promise<string[]> {
  const d = driver as { page: import("playwright").Page };
  const menu = d.page.getByRole("menu");
  await menu.getByRole("menuitem").first().waitFor({ timeout: 30_000 });
  const texts = await menu.getByRole("menuitem").allInnerTexts();
  return texts.map((text) => text.replace(/\s+/g, " ").trim()).filter((text) => text.length > 0);
}

async function pickDetailMenuItem(driver: unknown, name: string): Promise<void> {
  const d = driver as { page: import("playwright").Page };
  await d.page.getByRole("menuitem", { name }).first().click({ timeout: 30_000 });
  await d.page
    .getByRole("menu")
    .waitFor({ state: "hidden", timeout: 30_000 })
    .catch(() => undefined);
}

test(
  specTitle(
    ["VIEW-017", "VIEW-018", "VIEW-020"],
    "detail header menu follows ownership and copies the canonical address"
  ),
  { tag: specTags(["VIEW-017", "VIEW-018", "VIEW-020"]) },
  async ({ driver }) => {
    const harness = await viewsHarness("parity-vd17");
    const { owner, workspaceSlug, projectId } = harness;
    const name = `Vd17 Detail ${harness.tag}`;
    const created = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name });
    const member = await viewsSeat(harness, ROLE.MEMBER, "parity-vd17-member");
    const admin = await viewsSeat(harness, ROLE.ADMIN, "parity-vd17-admin");

    const cloud = driver.target === "web_new" && isCloudNewEdition();
    const ownerItems = cloud
      ? ["Edit", "Open in new tab", "Copy link", "Publish", "Delete"]
      : ["Edit", "Open in new tab", "Copy link", "Delete"];
    const adminItems = cloud
      ? ["Open in new tab", "Copy link", "Publish", "Delete"]
      : ["Open in new tab", "Copy link", "Delete"];

    await test.step("owners see edit and delete, members see subsets, admins see delete", async () => {
      await viewsOpenDetailAs(driver, harness, created, owner);
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
      await openDetailMenu(driver);
      expect(await detailMenuItems(driver)).toEqual(ownerItems);
      await driver.page.keyboard.press("Escape");

      await viewsOpenDetailAs(driver, harness, created, member);
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
      await openDetailMenu(driver);
      expect(await detailMenuItems(driver)).toEqual(["Open in new tab", "Copy link"]);
      await driver.page.keyboard.press("Escape");

      await viewsOpenDetailAs(driver, harness, created, admin);
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
      await openDetailMenu(driver);
      expect(await detailMenuItems(driver)).toEqual(adminItems);
      await driver.page.keyboard.press("Escape");
    });

    await test.step("copy link writes the canonical address even with query params", async () => {
      // Open with a peek param — the copy must still be the canonical address
      await driver.openAuthenticated(
        `/${workspaceSlug}/projects/${projectId}/views/${created}?peekIssueId=issue-9&layout=compact`,
        browserSessionCookies(owner)
      );
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
      await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await openDetailMenu(driver);
      await pickDetailMenuItem(driver, "Copy link");
      await expect.poll(() => driver.lastToast(), POLL).toContain("Link Copied");
      const clip = await driver.page.evaluate(() => navigator.clipboard.readText());
      expect(clip).toContain(`/projects/${projectId}/views/${created}`);
      // canonical: no peek/layout query params leaked
      expect(clip).not.toContain("peekIssueId");
      expect(clip).not.toContain("layout=compact");
      const detail = await serverProjectViewDetail(workspaceSlug, projectId, created, owner.cookie);
      expect(detail.id).toBe(created);
    });

    await test.step("open in new tab loads the same view at the canonical address", async () => {
      await viewsOpenDetailAs(driver, harness, created, owner);
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(name);
      await openDetailMenu(driver);
      const menu = driver.page.getByRole("menu");
      const [popup] = await Promise.all([
        driver.page.waitForEvent("popup", { timeout: 30_000 }),
        menu.getByRole("menuitem", { name: "Open in new tab" }).first().click({ timeout: 30_000 }),
      ]);
      expect(popup.url()).toContain(`/projects/${projectId}/views/${created}`);
      await popup.close().catch(() => undefined);
    });

    await test.step("delete from the detail navigates to the list", async () => {
      const doomed = `Vd17 Doomed ${harness.tag}`;
      const doomedId = await createProjectViewFull(workspaceSlug, projectId, owner.cookie, { name: doomed });
      await viewsOpenDetailAs(driver, harness, doomedId, owner);
      await expect.poll(() => driver.viewsDetailTabTitle(), POLL).toContain(doomed);
      await openDetailMenu(driver);
      await pickDetailMenuItem(driver, "Delete");
      await driver.viewsDeleteConfirm();
      await expect.poll(() => driver.lastToast(), POLL).toContain("View deleted successfully");
      // detail delete must return to the list, like the row menu does
      await expect.poll(() => driver.page.url(), POLL).toContain("/views");
      expect(driver.page.url()).not.toContain(doomedId);
    });
  }
);
