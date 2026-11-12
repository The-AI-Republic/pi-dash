// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): failed-connect bounce handling, row
// CLOUD-026 (an error parameter fires a single failure notification and
// is then stripped so later searches do not re-fire it), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): a connect that never started
// bounces back with an error parameter; loading the Apps page with one
// fires a single failure toast and strips the outcome parameters
// (error, connected) while keeping the rest, so a later search — which
// re-runs the effect — stays silent.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers";
import { cloudBrowserCookies, cloudDevLogin } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

async function toastCount(driver: ParityDriver): Promise<number> {
  return driver.page.locator('[aria-label="Notifications"]').getByRole("dialog").count();
}

test(
  specTitle(["CLOUD-026"], "error parameter reports once and cleans the URL"),
  { tag: specTags(["CLOUD-026"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    // Live disconnected page: no stubs — the bounce handling runs before
    // any catalog read matters.
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/apps?error=openhub_unavailable&connected=1&kept=yes`,
      cloudBrowserCookies(session.cookie)
    );
    await driver.dismissWelcomeDialog();
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe(`/${seed.workspaceSlug}/apps?kept=yes`);
    await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
    expect((await driver.lastToast()) ?? "").toContain("Connection failed");
    await driver.page.waitForTimeout(1_000);
    expect(await toastCount(driver)).toBe(1);
  }
);

test(
  specTitle(["CLOUD-026"], "later searches do not re-fire the stripped error"),
  { tag: specTags(["CLOUD-026"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([stubApp({ appId: "app-notes", slug: "notes", name: "Notes Pro" })]),
        }),
    });
    try {
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/apps?error=openhub_unavailable`,
        cloudBrowserCookies(session.cookie)
      );
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe(`/${seed.workspaceSlug}/apps`);
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      // A new search re-runs the bounce effect (new fetch key): silence
      // proves the parameter is gone, not merely unhandled.
      await driver.cloudAppsSearchFill("notes");
      await expect
        .poll(() => stub.log.filter((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/")).length, {
          timeout: 60_000,
        })
        .toBeGreaterThan(1);
      await driver.page.waitForTimeout(1_000);
      expect(await toastCount(driver)).toBe(1);
      expect(await driver.currentPath()).toBe(`/${seed.workspaceSlug}/apps`);
    } finally {
      await stub.uninstall();
    }
  }
);
