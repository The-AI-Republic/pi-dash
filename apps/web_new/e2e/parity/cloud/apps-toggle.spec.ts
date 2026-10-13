// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): agent-tools toggle, row CLOUD-029
// (one switch governs whether agents may use installed apps,
// optimistic with rollback on refusal), green on the cloud-edition old
// app first. The same file must go green on apps/web_new once the
// cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): the switch states plainly
// that enabling exposes every installed app's tools — including
// mutating ones — to every agent run the user starts; flipping it
// patches the namespaced profile-settings flag, applies immediately,
// and persists; a refused write rolls the switch back with an error
// notice and the server value unchanged. The profile endpoints are
// live (no gateway involved).
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudProfilePatchRaw, cloudProfileRaw } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage } from "./openhub-stub";

async function agentToolsFlag(cookie: string): Promise<boolean> {
  const profile = await cloudProfileRaw(cookie);
  expect(profile.status).toBe(200);
  const settings = (profile.payload["settings"] ?? {}) as Record<string, unknown>;
  const openhub = (settings["openhub"] ?? {}) as Record<string, unknown>;
  return openhub["apps_enabled"] === true;
}

test(
  specTitle(["CLOUD-029"], "toggle persists the agent-tools preference live"),
  { tag: specTags(["CLOUD-029"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    // Convergent start: the seed default is off, a retry may find it on.
    await cloudProfilePatchRaw(session.cookie, { settings: { openhub: { apps_enabled: false } } });
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([]) }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(false);
      // The control discloses the whole privilege boundary up front.
      const text = (await driver.cloudAppsToggleText()).toLowerCase();
      expect(text).toContain("agents");
      expect(text).toContain("tools");
      expect(text).toContain("create");
      expect(text).toContain("delete");

      await driver.cloudAppsToggle();
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(true);
      // The switch is optimistic: the UI flips before the profile PATCH
      // lands, so the server read gets a deadline instead of asserting once.
      await expect.poll(() => agentToolsFlag(session.cookie), { timeout: 60_000 }).toBe(true);
      await driver.page.reload({ waitUntil: "domcontentloaded" });
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(true);

      await driver.cloudAppsToggle();
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(false);
      await expect.poll(() => agentToolsFlag(session.cookie), { timeout: 60_000 }).toBe(false);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-029"], "refused write rolls the toggle back with an error"),
  { tag: specTags(["CLOUD-029"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    await cloudProfilePatchRaw(session.cookie, { settings: { openhub: { apps_enabled: false } } });
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([]) }),
    });
    // Every PATCH refuses (GETs continue live): the load read must still
    // succeed, so no `times` cap — the first call here is the GET.
    await driver.page.context().route("**/api/users/me/profile/", (route) => {
      if (route.request().method() === "PATCH") {
        return route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ error: "profile_write_failed" }),
        });
      }
      return route.continue();
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(false);
      await driver.cloudAppsToggle();
      // Optimistic on, then rolled back when the server refuses.
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      expect((await driver.lastToast()) ?? "").toContain("Could not save");
      await expect.poll(() => driver.cloudAppsToggleState(), { timeout: 60_000 }).toBe(false);
      expect(await agentToolsFlag(session.cookie)).toBe(false);
    } finally {
      await driver.page
        .context()
        .unroute("**/api/users/me/profile/")
        .catch(() => undefined);
      await stub.uninstall();
    }
  }
);
