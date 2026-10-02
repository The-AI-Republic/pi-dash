// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3b. DOM dumps for the peek title
// element, the delete dialog text, and non-manual drop release behavior.
// Committed as a resume aid; deleted before PR.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/index";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserCookies,
  serverCreateIssue,
  serverDeleteIssue,
  serverIssueDetails,
  serverIssues,
  serverListStates,
  serverPatchIssue,
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

test("probe34b: peek panel title element", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state" });
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
  await page.getByRole("main").locator('a[id^="issue_"]').nth(1).click({ timeout: 30_000 });
  await page.waitForTimeout(5_000);
  const panel = page.locator("div.absolute.top-0.right-0.bottom-0").last();
  const inputs = await panel
    .locator("input, textarea")
    .evaluateAll((els) => els.map((el) => `${el.tagName}:${(el as HTMLInputElement).value ?? ""}`.slice(0, 120)));
  console.log(`P34B-INPUTS:${JSON.stringify(inputs).slice(0, 1000)}`);
  const text = ((await panel.innerText().catch(() => "")) ?? "").split("\n").slice(0, 14);
  console.log(`P34B-LINES:${JSON.stringify(text).slice(0, 1000)}`);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});

test("probe40b: delete dialog text after zone drop", async ({ driver, seed, page }) => {
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
  const card = page.getByRole("main").locator('a[id^="issue_"]').last();
  const box = await card.boundingBox();
  const zoneBox = await zone.first().boundingBox();
  if (box && zoneBox) {
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(zoneBox.x + zoneBox.width / 2, zoneBox.y + zoneBox.height / 2, { steps: 10 });
    await page.waitForTimeout(800);
    await page.mouse.up();
    await page.waitForTimeout(3_000);
  }
  const dialogs = page.getByRole("dialog");
  console.log(`P40B-DIALOGS:${await dialogs.count()}`);
  for (let i = 0; i < (await dialogs.count()); i += 1) {
    const t = await dialogs
      .nth(i)
      .innerText()
      .catch(() => "");
    console.log(`P40B-DIALOG-${i}:${JSON.stringify(t).slice(0, 500)}`);
  }
  console.log(`P40B-CARDS:${JSON.stringify(await driver.kanbanCards()).slice(0, 300)}`);
  await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie).catch(() => undefined);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});

test("probe37b: non-manual drop release behavior", async ({ driver, seed, page }) => {
  test.setTimeout(300_000);
  const cookie = await openSeedBoard(driver, seed, seed.projectId, { group_by: "state", order_by: "priority" });
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
  const user = await signInFreshUser(seed.email, seed.password);
  const orderOf = async (name: string): Promise<number> => {
    const rows = await serverIssues(seed.workspaceSlug, seed.projectId, user.cookie);
    const found = rows.find((r) => r.name === name);
    if (!found) throw new Error("[probe] missing issue");
    return (await serverIssueDetails(seed.workspaceSlug, seed.projectId, found.id, user.cookie)).sortOrder;
  };
  const cards = await driver.kanbanCards();
  const target = cards[0]?.name ?? "";
  const dragged = cards[2]?.name ?? "";
  const before = await orderOf(dragged);
  const card = page.getByRole("main").locator('a[id^="issue_"]').nth(2);
  const dest = page.getByRole("main").locator('a[id^="issue_"]').nth(0);
  const from = await card.boundingBox();
  const onto = await dest.boundingBox();
  if (!from || !onto) throw new Error("[probe] no boxes");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(onto.x + onto.width / 2, onto.y + 4, { steps: 10 });
  await page.waitForTimeout(800);
  await page.mouse.up();
  await page.waitForTimeout(3_000);
  console.log(`P37B-CARDS:${JSON.stringify((await driver.kanbanCards()).map((c) => c.name))}`);
  console.log(`P37B-ORDER-SAME:${(await orderOf(dragged)) === before}`);
  console.log(`P37B-TOAST:${JSON.stringify(await driver.boardLastToast())}`);
  console.log(`P37B-TARGET:${target} DRAGGED:${dragged}`);
  const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...props.displayFilters, layout: "list", group_by: null, order_by: "sort_order" },
  });
  // Restore manual order in case the drop reordered.
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, user.cookie);
  const ranks = [15000, 25000, 35000];
  for (const [i, n] of seed.issueNames.entries()) {
    const row = rows.find((r) => r.name === n);
    if (row)
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, row.id, { sort_order: ranks[i] ?? 0 }, user.cookie);
  }
});
