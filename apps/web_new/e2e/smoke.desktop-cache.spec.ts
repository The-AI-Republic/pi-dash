// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop smoke (NEWFRONT-18): launch the desktop bundle (PIDASH_TARGET=
// desktop, served by the desktop webServer in playwright.config.ts), sign
// in, open the issue list, then relaunch with the issues API blocked and
// prove the list still renders from the persisted query cache. Skips
// without PIDASH_E2E_BASE_URL (same contract seed as smoke.signin-issues).
import { expect, test } from "@playwright/test";

const API_BASE_URL = process.env["PIDASH_E2E_BASE_URL"] ?? "";
const DESKTOP_URL = process.env["PIDASH_E2E_DESKTOP_APP_URL"] ?? "http://localhost:3011";
const EMAIL = process.env["PIDASH_E2E_EMAIL"] ?? "";
const PASSWORD = process.env["PIDASH_E2E_PASSWORD"] ?? "";
const WORKSPACE = process.env["PIDASH_E2E_WORKSPACE"] ?? "";
const PROJECT_ID = process.env["PIDASH_E2E_PROJECT_ID"] ?? "";

const enabled =
  API_BASE_URL.length > 0 && EMAIL.length > 0 && PASSWORD.length > 0 && WORKSPACE.length > 0 && PROJECT_ID.length > 0;

const ISSUES_PATH = `/api/workspaces/${WORKSPACE}/projects/${PROJECT_ID}/issues/`;

/** Bytes stored under the desktop query-cache key (IndexedDB, `pidash.desktop` namespace). */
function desktopCacheBytes(): Promise<number> {
  return new Promise((resolve) => {
    try {
      const open = globalThis.indexedDB.open("pidash-web-new", 1);
      open.onsuccess = () => {
        const db = open.result;
        if (!db.objectStoreNames.contains("keyvalue")) {
          db.close();
          resolve(0);
          return;
        }
        const request = db
          .transaction("keyvalue", "readonly")
          .objectStore("keyvalue")
          .get("pidash.desktop:tanstack-query-cache");
        request.onsuccess = () => {
          const value: unknown = request.result;
          db.close();
          resolve(typeof value === "string" ? value.length : 0);
        };
        request.onerror = () => {
          db.close();
          resolve(0);
        };
      };
      open.onerror = () => resolve(0);
    } catch {
      resolve(0);
    }
  });
}

test.describe("desktop launch to cached relaunch", () => {
  test.skip(!enabled, "needs PIDASH_E2E_BASE_URL/EMAIL/PASSWORD/WORKSPACE/PROJECT_ID");

  test("signs in, lists issues, relaunches from cache with the API blocked", async ({ page }) => {
    await page.goto(`${DESKTOP_URL}/sign-in`);
    await expect(page.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();

    await page.getByLabel("Work email").fill(EMAIL);
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(page.getByLabel("Password")).toBeVisible();
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Sign in" }).click();
    await expect(page).not.toHaveURL(/sign-in/);

    await page.goto(`${DESKTOP_URL}/${WORKSPACE}/projects/${PROJECT_ID}/issues`);
    await expect(page.getByRole("region", { name: "Issues" })).toBeVisible();
    const seededRow = page.getByRole("article").filter({ hasText: "Contract issue one" });
    await expect(seededRow).toBeVisible();

    // The served bundle is the desktop target: the title bar is a drag
    // region and the location is mirrored in the document title.
    await expect(page.locator("header[data-tauri-drag-region]")).toBeVisible();
    await expect.poll(() => page.title(), { timeout: 15_000 }).toContain("Pi Dash");

    // Wait for the debounced cache write before simulating the relaunch:
    // without a persisted payload the reload below proves nothing.
    await expect.poll(() => page.evaluate(desktopCacheBytes), { timeout: 15_000 }).toBeGreaterThan(0);

    // Relaunch with the network gone for the issue list. Session and
    // reference data may still load live; the rows must come from cache.
    await page.route(`**${ISSUES_PATH}**`, (route) => route.abort());
    await page.reload();
    await expect(page.getByRole("region", { name: "Issues" })).toBeVisible();
    await expect(page.getByRole("article").filter({ hasText: "Contract issue one" })).toBeVisible();
  });
});
