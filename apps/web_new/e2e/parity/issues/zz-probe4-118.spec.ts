// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 3c. Non-manual drop over empty column
// space, and delete-modal locator semantics. Resume aid; deleted before PR.
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

test("probe37c: non-manual drop over empty column space", async ({ driver, seed, page }) => {
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
  const dragged = cards[0]?.name ?? "";
  const before = await orderOf(dragged);
  const card = page.getByRole("main").locator('a[id^="issue_"]').nth(0);
  const colOuter = page.getByRole("main").locator("div.group.relative.flex.flex-shrink-0.flex-col").first();
  const from = await card.boundingBox();
  const colBox = await colOuter.boundingBox();
  if (!from || !colBox) throw new Error("[probe] no boxes");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  // Bottom of the column body, below the last card (empty space).
  await page.mouse.move(colBox.x + colBox.width / 2, colBox.y + colBox.height - 40, { steps: 10 });
  await page.waitForTimeout(1_000);
  const hint = page.getByRole("main").locator('div[id$="__null"] > div').first();
  console.log(`P37C-HINT:${JSON.stringify(await hint.innerText().catch(() => null))}`);
  await page.mouse.up();
  await page.waitForTimeout(3_000);
  console.log(`P37C-CARDS:${JSON.stringify((await driver.kanbanCards()).map((c) => c.name))}`);
  console.log(`P37C-ORDER-SAME:${(await orderOf(dragged)) === before} BEFORE:${before}`);
  console.log(`P37C-TOAST:${JSON.stringify(await driver.boardLastToast())}`);
  console.log(`P37C-DRAGGED:${dragged}`);
  const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...props.displayFilters, layout: "list", group_by: null, order_by: "sort_order" },
  });
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, user.cookie);
  const ranks = [15000, 25000, 35000];
  for (const [i, n] of seed.issueNames.entries()) {
    const row = rows.find((r) => r.name === n);
    if (row)
      await serverPatchIssue(seed.workspaceSlug, seed.projectId, row.id, { sort_order: ranks[i] ?? 0 }, user.cookie);
  }
});

test("probe40c: delete modal locator semantics", async ({ driver, seed, page }) => {
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
  const filtered = page.getByRole("dialog").filter({ hasText: "Delete Work item" });
  console.log(`P40C-FILTERED-COUNT:${await filtered.count()}`);
  if ((await filtered.count()) > 0) {
    console.log(`P40C-FILTERED-VISIBLE:${await filtered.first().isVisible()}`);
    console.log(`P40C-FILTERED-BOX:${JSON.stringify(await filtered.first().boundingBox())}`);
  }
  const exact = page.getByText("Delete Work item", { exact: true });
  console.log(`P40C-EXACT-COUNT:${await exact.count()}`);
  if ((await exact.count()) > 0) console.log(`P40C-EXACT-VISIBLE:${await exact.first().isVisible()}`);
  console.log(`P40C-DRIVER-SAYS:${await driver.kanbanDeleteModalVisible()}`);
  const dialogs = page.getByRole("dialog");
  for (let i = 0; i < (await dialogs.count()); i += 1) {
    console.log(
      `P40C-DLG-${i}-ROLE:${await dialogs.nth(i).getAttribute("role")} TEXT:${JSON.stringify(
        await dialogs
          .nth(i)
          .innerText()
          .catch(() => "")
      ).slice(0, 200)}`
    );
  }
  // Close whatever opened, then clean up.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(2_000);
  await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie).catch(() => undefined);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, cookie, {
    display_filters: { ...before.displayFilters, layout: "list", group_by: null },
  });
});
