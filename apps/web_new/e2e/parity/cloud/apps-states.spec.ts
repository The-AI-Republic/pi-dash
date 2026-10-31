// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): catalog failure states, row
// CLOUD-030 (stale sessions, unreachable service, catalog failures
// with inline retry, distinct empty states), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): a session that predates
// catalog access names sign-out-and-back-in with no catalog chrome;
// a scoped-but-disconnected session shows the server detail (or a
// generic unavailable line); a status read that never answers shows
// its error with a retry that re-fires the read; a failing catalog
// read shows an inline retry and never masquerades as an empty
// catalog; empty states differ between no apps available and no
// search matches, echoing the query.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudOpenhubStatusRaw } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

test(
  specTitle(["CLOUD-030"], "stale sessions point at sign-out-and-back-in"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    // Live server state behind scratch: disconnected, pre-scopes.
    const live = await cloudOpenhubStatusRaw(session.cookie);
    expect(live.status).toBe(200);
    expect(live.payload["connected"]).toBe(false);
    expect(live.payload["has_gateway_scopes"]).toBe(false);
    expect(typeof live.payload["hub_url"]).toBe("string");

    // Live page, no stubs: the real disconnected shape.
    await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
    await driver.dismissWelcomeDialog();
    // Settle on content, not chrome: the "Apps" heading renders while the
    // status read still flies on web_new, so polling for it passes early.
    await expect
      .poll(async () => (await driver.cloudAppsBodyText()).toLowerCase(), { timeout: 60_000 })
      .toContain("sign out");
    const body = (await driver.cloudAppsBodyText()).toLowerCase();
    expect(body).toContain("sign out");
    expect(body).toContain("back in");
    expect(await driver.cloudAppsSearchVisible()).toBe(false);
    expect(await driver.cloudAppsCatalogVisible()).toBe(false);
    expect(await driver.cloudAppsToggleState()).toBeNull();
  }
);

test(
  specTitle(["CLOUD-030"], "scoped-but-disconnected sessions show the server detail"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let detail: string | undefined = "Hub is down for maintenance.";
    const stub = await installOpenhubStub(driver.page.context(), {
      status: () =>
        Promise.resolve({
          status: 200,
          body: {
            connected: false,
            has_gateway_scopes: true,
            hub_url: "https://hub.airepublic.com",
            ...(detail === undefined ? {} : { detail }),
          },
        }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("Hub is down");
      expect(await driver.cloudAppsCatalogVisible()).toBe(false);

      detail = undefined;
      await driver.page.reload({ waitUntil: "domcontentloaded" });
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("unavailable");
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-030"], "unreachable status explains itself with a retry"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let calls = 0;
    const stub = await installOpenhubStub(driver.page.context(), {
      // The first status read fails named; the retry continues live and
      // lands on the real stale-session panel.
      status: () => {
        calls += 1;
        if (calls === 1) {
          return Promise.resolve({
            status: 500,
            body: { error: "openhub_error", detail: "Status check blew up." },
          });
        }
        return "continue";
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("Status check blew up");
      await driver.cloudAppsClickRetry();
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("Sign out and back in");
      expect(calls).toBeGreaterThanOrEqual(2);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-030"], "bug: NEWFRONT-473 aborted status read masquerades as an empty catalog"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      status: () => "abort",
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      if (driver.target === "web_new") {
        // Intended (NEWFRONT-473): the aborted read renders the
        // status-error panel with its Retry control instead of falling
        // through to an empty catalog.
        await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("Apps are unavailable");
        expect(await driver.cloudAppsCatalogVisible()).toBe(false);
        await driver.cloudAppsClickRetry();
        await expect
          .poll(() => stub.log.filter((entry) => entry.path.startsWith("/api/openhub/status/")).length, {
            timeout: 60_000,
          })
          .toBeGreaterThan(1);
        expect(stub.log.some((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/"))).toBe(false);
        return;
      }
      // Current (buggy) behavior: full catalog chrome over the empty
      // state, no error, no retry. Intended: the status-error panel
      // with its Retry control (see NEWFRONT-473).
      await expect.poll(() => driver.cloudAppsSearchVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.cloudAppsToggleState()).not.toBeNull();
      expect(await driver.cloudAppsCatalogVisible()).toBe(false);
      const retry = driver.page.locator("main main").last().getByRole("button", { name: "Retry" });
      expect(await retry.count()).toBe(0);
      expect(stub.log.some((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/"))).toBe(false);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-030"], "catalog failures retry inline and never masquerade as empty"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let calls = 0;
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => {
        calls += 1;
        if (calls === 1) {
          return Promise.resolve({
            status: 503,
            body: { error: "openhub_unavailable", detail: "Catalog is unreachable." },
          });
        }
        return Promise.resolve({
          status: 200,
          body: marketplacePage([stubApp({ appId: "app-back", slug: "back", name: "Back Online" })]),
        });
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("Catalog is unreachable");
      expect(await driver.cloudAppsCatalogVisible()).toBe(false);
      // An error, not an empty catalog: it carries a retry. The old app
      // nests a second main around the panel; web_new keeps one.
      const scope =
        driver.target === "web_new" ? driver.page.locator("main").last() : driver.page.locator("main main").last();
      const retry = scope.getByRole("button", { name: "Retry" });
      expect(await retry.count()).toBe(1);
      await driver.cloudAppsClickRetry();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Back Online"]);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-030"], "empty states distinguish no apps from no matches"),
  { tag: specTags(["CLOUD-030"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([]) }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsSearchVisible(), { timeout: 60_000 }).toBe(true);
      const emptyCatalog = await driver.cloudAppsBodyText();
      expect(await driver.cloudAppsCatalogVisible()).toBe(false);

      await driver.cloudAppsSearchFill("zzz-no-such-app");
      await expect.poll(() => driver.cloudAppsBodyText(), { timeout: 60_000 }).toContain("zzz-no-such-app");
      const noMatches = await driver.cloudAppsBodyText();
      expect(noMatches).not.toBe(emptyCatalog);
      // Neither is a failure: no retry in either state.
      const retry = driver.page.locator("main main").last().getByRole("button", { name: "Retry" });
      expect(await retry.count()).toBe(0);
    } finally {
      await stub.uninstall();
    }
  }
);
