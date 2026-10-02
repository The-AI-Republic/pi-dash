// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3. Dumps live DOM facts for the five
// kanban failures without an obvious fix (ISS-030/034/037/040/042).
// Committed as a resume aid; deleted before PR.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/index";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverDeleteIssue,
  serverDeleteProject,
  serverIssues,
  serverListStates,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

async function openSeedBoard(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  filters: Record<string, unknown>
): Promise<string> {
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      sub_group_by: null,
      show_empty_groups: true,
      ...filters,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await driver.kanbanOpenBoard();
  return user.cookie;
}

test("probe30: collapsed column across reload", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state" });
  await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).not.toHaveLength(0);
  const name = (await driver.kanbanColumns())[0]?.name ?? "";
  await driver.kanbanToggleColumn(name);
  console.log(`P30-COLLAPSED:${await driver.kanbanColumnCollapsed(name)}`);
  console.log(`P30-VISIBLE-BEFORE-RELOAD:${await driver.kanbanBoardVisible()}`);
  console.log(`P30-BODIES-BEFORE:${await page.getByRole("main").locator('div[id*="__"]').count()}`);
  await driver.boardReloadIssues();
  await page.waitForTimeout(5_000);
  console.log(`P30-VISIBLE-AFTER-RELOAD:${await driver.kanbanBoardVisible()}`);
  console.log(`P30-BODIES-AFTER:${await page.getByRole("main").locator('div[id*="__"]').count()}`);
  console.log(`P30-COLUMNS-AFTER:${JSON.stringify(await driver.kanbanColumns())}`);
  await driver.kanbanToggleColumn(name);
  console.log(`P30-EXPANDED:${await driver.kanbanColumnCollapsed(name)}`);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});

test("probe34: what a card click opens", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state" });
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
  const card = page.getByRole("main").locator('a[id^="issue_"]').nth(1);
  console.log(`P34-HREF:${await card.getAttribute("href")} TARGET:${await card.getAttribute("target")}`);
  await card.click({ timeout: 30_000 });
  await page.waitForTimeout(5_000);
  console.log(`P34-PEEK-VISIBLE:${await driver.issuePeekVisible()}`);
  console.log(`P34-PEEK-TITLE:${JSON.stringify(await driver.issuePeekTitle())}`);
  console.log(`P34-URL:${page.url()}`);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});

test("probe37: overlay texts during a non-manual hold", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state", order_by: "priority" });
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
  const cards = await driver.kanbanCards();
  const first = cards[0]?.name ?? "";
  const column = (await driver.kanbanColumns())[0]?.name ?? "";
  // Manual hold: press the card, hover the column center, dump, release.
  const card = page.getByRole("main").locator('a[id^="issue_"]').first();
  const box = await card.boundingBox();
  const colOuter = page.getByRole("main").locator("div.group.relative.flex.flex-shrink-0.flex-col").first();
  const colBox = await colOuter.boundingBox();
  if (!box || !colBox) throw new Error("[probe] no boxes");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(colBox.x + colBox.width / 2, colBox.y + colBox.height / 2, { steps: 8 });
  await page.waitForTimeout(1_000);
  const hint = page.getByRole("main").locator('div[id$="__null"] > div').first();
  console.log(`P37-HINT-COUNT:${await hint.count()}`);
  if ((await hint.count()) > 0) {
    console.log(`P37-HINT-CLASS:${await hint.getAttribute("class")}`);
    console.log(`P37-HINT-TEXT:${JSON.stringify(await hint.innerText().catch(() => null))}`);
  }
  const bodies = await page
    .getByRole("main")
    .locator('div[id*="__"]')
    .evaluateAll((els) => els.map((el) => (el.textContent ?? "").slice(0, 120)));
  console.log(`P37-BODIES:${JSON.stringify(bodies).slice(0, 800)}`);
  const orderish = await page.locator("body").evaluateAll((els) =>
    els
      .flatMap((el) => Array.from(el.querySelectorAll("*")))
      .map((el) => (el.textContent ?? "").trim())
      .filter((t) => /order|priorit|sorted|manual/i.test(t) && t.length < 160)
      .slice(0, 12)
  );
  console.log(`P37-ORDERISH:${JSON.stringify(orderish).slice(0, 1200)}`);
  await page.mouse.up();
  await page.waitForTimeout(1_500);
  console.log(`P37-FIRST:${first} COLUMN:${column}`);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null, order_by: "sort_order" },
  });
});

test("probe40: delete zone drop and modal", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const states = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  const title = `KB doomed ${uniqueSuffix().slice(0, 6)}`;
  const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, owner.cookie, title, home.id);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state" });
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(4);
  const zone = page.getByText("Drop here to delete", { exact: false });
  console.log(`P40-ZONE-COUNT:${await zone.count()}`);
  const card = page.getByRole("main").locator('a[id^="issue_"]').last();
  const box = await card.boundingBox();
  const zoneBox = await zone.first().boundingBox();
  console.log(`P40-ZONE-BOX:${JSON.stringify(zoneBox)}`);
  if (box && zoneBox) {
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(zoneBox.x + zoneBox.width / 2, zoneBox.y + zoneBox.height / 2, { steps: 10 });
    await page.waitForTimeout(1_000);
    console.log(
      `P40-ZONE-TEXT-DURING:${JSON.stringify(
        await zone
          .first()
          .innerText()
          .catch(() => null)
      )}`
    );
    await page.mouse.up();
    await page.waitForTimeout(3_000);
  }
  console.log(`P40-MODAL:${await driver.kanbanDeleteModalVisible()}`);
  console.log(`P40-TOAST:${JSON.stringify(await driver.boardLastToast())}`);
  console.log(`P40-DIALOGS:${await page.getByRole("dialog").count()}`);
  await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie).catch(() => undefined);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});

test("probe42: scratch column with 32 issues", async ({ driver, seed }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KX${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  console.log(`P42-HOME:${home.name} DEFAULT:${home.isDefault} STATES:${states.map((s) => s.name).join(",")}`);
  for (let i = 0; i < 32; i += 1) {
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe ${suffix} ${i + 1}`, home.id);
  }
  console.log(`P42-SERVER:${(await serverIssues(seed.workspaceSlug, projectId, owner.cookie)).length}`);
  await openSeedBoard(driver, seed, projectId, { group_by: "state" });
  await driver.boardReloadIssues();
  await driver.kanbanOpenBoard();
  console.log(`P42-COLUMNS:${JSON.stringify(await driver.kanbanColumns())}`);
  console.log(`P42-CARDS:${(await driver.kanbanColumnCards(home.name)).length}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
});
