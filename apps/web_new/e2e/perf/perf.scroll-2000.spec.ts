// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scroll budget (NEWFRONT-21, Quality gates): an issue list with 2,000 rows
// scrolls with no frame over 50 ms. Self-contained: the web production
// bundle (serve.mjs webServer below) with the issues API mocked to 2,000
// rows, so CI needs no backend. The recorder samples real rAF pacing while
// the list scrolls top to bottom; a virtualized list keeps ~30 rows mounted
// and every frame cheap.
import { expect, test } from "@playwright/test";

import { installApiMocks, perfIssuesUrl } from "./mocks";

const WEB_URL = process.env["PERF_WEB_URL"] ?? "http://localhost:3021";
const FRAME_BUDGET_MS = 50;
const ROW_COUNT = 2000;
const SCROLL_STEPS = 120;

test("issue list scrolls 2000 rows with no frame over 50 ms", async ({ page }) => {
  await installApiMocks(page, { issueCount: ROW_COUNT });

  await page.goto(`${WEB_URL}${perfIssuesUrl()}`);
  const region = page.getByRole("region", { name: "Issues" });
  await expect(region.getByRole("article").first()).toBeVisible();
  const rowCount = await region.getByRole("article").count();
  expect(rowCount).toBeGreaterThan(0);
  expect(rowCount).toBeLessThan(ROW_COUNT);

  await page.evaluate(() => {
    const state = globalThis as unknown as { __perfFrames: number[]; __perfStop: () => void };
    const frames: number[] = [];
    state.__perfFrames = frames;
    let last = performance.now();
    let running = true;
    state.__perfStop = () => {
      running = false;
    };
    const tick = () => {
      if (!running) return;
      const now = performance.now();
      frames.push(now - last);
      last = now;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  for (let step = 0; step <= SCROLL_STEPS; step++) {
    await region.evaluate((element, ratio) => {
      element.scrollTop = (element.scrollHeight - element.clientHeight) * ratio;
    }, step / SCROLL_STEPS);
    await page.waitForTimeout(16);
  }

  const maxFrameMs = await page.evaluate(() => {
    const state = globalThis as unknown as { __perfFrames: number[]; __perfStop: () => void };
    state.__perfStop();
    return state.__perfFrames.length > 0 ? Math.max(...state.__perfFrames) : Number.NaN;
  });
  expect(Number.isNaN(maxFrameMs)).toBe(false);
  console.log(`[perf] 2000-row scroll max frame: ${maxFrameMs.toFixed(1)} ms (budget ${FRAME_BUDGET_MS} ms)`);
  expect(maxFrameMs).toBeLessThanOrEqual(FRAME_BUDGET_MS);
});
