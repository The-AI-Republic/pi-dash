// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop warm-start budget (NEWFRONT-21, Quality gates): first meaningful
// paint from a warm cache renders in ≤ 500 ms. Self-contained: the desktop
// production bundle (serve.mjs webServer below) with the API mocked, so CI
// needs no backend. A cold load populates the persisted query cache; the
// measured reload blocks the issues API, proving the rows come from the
// warm cache and not the network.
//
// The relaunch happens only once the stored payload provably carries the
// issue list (NEWFRONT-886). The persistor writes a debounced, settled-only
// snapshot, so an early snapshot can hold the bootstrap queries alone; the
// byte length says nothing about whether rows survived, and reloading on
// such a payload would only paint the blocked-API error state.
import { expect, test } from "@playwright/test";

import { cachedIssueListRows } from "./cachedIssues.js";
import { installApiMocks, PERF_PROJECT_ID, perfIssuesUrl } from "./mocks";

const DESKTOP_URL = process.env["PERF_DESKTOP_URL"] ?? "http://localhost:3022";
const WARM_START_BUDGET_MS = 500;

/** The stored desktop query-cache payload, or null while there is none. Runs in the page. */
function desktopCachePayload(): Promise<string | null> {
  return new Promise((resolve) => {
    try {
      const open = globalThis.indexedDB.open("pidash-web-new", 1);
      open.onsuccess = () => {
        const db = open.result;
        if (!db.objectStoreNames.contains("keyvalue")) {
          db.close();
          resolve(null);
          return;
        }
        const request = db
          .transaction("keyvalue", "readonly")
          .objectStore("keyvalue")
          .get("pidash.desktop:tanstack-query-cache");
        request.onsuccess = () => {
          const value: unknown = request.result;
          db.close();
          resolve(typeof value === "string" ? value : null);
        };
        request.onerror = () => {
          db.close();
          resolve(null);
        };
      };
      open.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

test("desktop warm start renders cached rows within 500 ms", async ({ page }) => {
  await installApiMocks(page, { issueCount: 3 });

  await page.goto(`${DESKTOP_URL}${perfIssuesUrl()}`);
  const rows = page.getByRole("region", { name: "Issues" }).getByRole("article");
  await expect(rows.first()).toBeVisible();

  // Wait for the debounced cache write before the measured relaunch, and for
  // it to carry this project's rows: without the list entry the reload below
  // can only paint the blocked-API error state.
  await expect
    .poll(async () => cachedIssueListRows(await page.evaluate(desktopCachePayload), PERF_PROJECT_ID), {
      timeout: 15_000,
    })
    .toBeGreaterThan(0);

  // Relaunch with the issues API gone. Rows can only come from the cache.
  await page.unrouteAll();
  await installApiMocks(page, { issueCount: 3, blockIssues: true });

  const started = Date.now();
  await page.reload();
  await expect(rows.first()).toBeVisible();
  const warmMs = Date.now() - started;
  console.log(`[perf] desktop warm start: ${warmMs} ms (budget ${WARM_START_BUDGET_MS} ms)`);
  expect(warmMs).toBeLessThanOrEqual(WARM_START_BUDGET_MS);
});
