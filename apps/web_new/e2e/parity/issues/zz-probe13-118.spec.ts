// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. Identify the board horizontal
// scroller; sample scroll during an edge hold. Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateProject,
  serverCreateState,
  serverDeleteProject,
  serverListStates,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe13: board scroller identity + hold sampling", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  for (let extra = states.length; extra < 8; extra += 1) {
    await serverCreateState(seed.workspaceSlug, projectId, owner.cookie, `KB pad ${suffix} ${extra}`, "unstarted");
  }
  for (let i = 0; i < 12; i += 1) {
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe ${suffix} ${i + 1}`, home.id);
  }
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  const scrollers = await page.evaluate(() => {
    const probe = document.querySelector('main div[id*="__"]');
    const chain: string[] = [];
    let node: HTMLElement | null = probe instanceof HTMLElement ? probe : null;
    while (node) {
      const h = node.scrollWidth > node.clientWidth + 4;
      const v = node.scrollHeight > node.clientHeight + 4;
      if (h || v) {
        chain.push(
          `${node.tagName}.${(node.className.baseVal ?? node.className).toString().slice(0, 60)} h=${h}(${node.scrollWidth}x${node.clientWidth}@${node.scrollLeft}) v=${v}`
        );
      }
      node = node.parentElement;
    }
    const doc = document.scrollingElement as HTMLElement | null;
    return {
      chain,
      doc: doc ? `${doc.tagName} ${doc.scrollWidth}x${doc.clientWidth}@${doc.scrollLeft}` : "none",
      viewport: `${window.innerWidth}x${window.innerHeight}`,
    };
  });
  console.log(`P13-CHAIN:${JSON.stringify(scrollers)}`);
  console.log(`P13-READ:${JSON.stringify(await driver.kanbanBoardScroll())}`);
  // Drive the host directly: does the reader follow?
  await page.evaluate(() => {
    const probe = document.querySelector('main div[id*="__"]');
    let node: HTMLElement | null = probe instanceof HTMLElement ? probe : null;
    while (node) {
      const axis = window.getComputedStyle(node).overflowX;
      if (node.scrollWidth > node.clientWidth + 4 && (axis === "auto" || axis === "scroll")) {
        node.scrollLeft = 300;
        break;
      }
      node = node.parentElement;
    }
  });
  console.log(`P13-AFTER-SET:${JSON.stringify(await driver.kanbanBoardScroll())}`);
  await page.evaluate(() => {
    const probe = document.querySelector('main div[id*="__"]');
    let node: HTMLElement | null = probe instanceof HTMLElement ? probe : null;
    while (node) {
      const axis = window.getComputedStyle(node).overflowX;
      if (node.scrollWidth > node.clientWidth + 4 && (axis === "auto" || axis === "scroll")) {
        node.scrollLeft = 0;
        break;
      }
      node = node.parentElement;
    }
  });
  // Sample the reader during a real hold.
  const contentful = (await driver.kanbanColumnCards(home.name)).filter((n) => n !== "");
  const held = contentful[Math.floor(contentful.length / 2)] ?? "";
  console.log(`P13-HELD:${held} contentful=${contentful.length}`);
  const card = page.locator('a[id^="issue_"]', { hasText: held }).first();
  const box = await card.boundingBox();
  console.log(`P13-BOX:${JSON.stringify(box)}`);
  if (box) {
    const viewport = page.viewportSize() ?? { width: 1280, height: 720 };
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(viewport.width - 12, box.y + box.height / 2, { steps: 10 });
    for (let i = 0; i < 6; i += 1) {
      await page.waitForTimeout(500);
      console.log(`P13-HOLD${i}:${JSON.stringify(await driver.kanbanBoardScroll())}`);
    }
    await page.mouse.up();
  }
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P13-DONE");
});
