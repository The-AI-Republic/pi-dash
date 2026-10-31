// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): provider OAuth handoff + hub detail
// links, rows CLOUD-025 (connects run in a pre-opened tab) and
// CLOUD-031 (connected provider apps open in the app hub), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): the connect tab opens
// synchronously on click with placeholder content and a detached
// opener, then navigates to the same-origin begin URL carrying
// return_path; the server 302s to consent (live: back to return_path
// with an error when the handoff cannot start, which the tab reports
// and strips); Pi Dash itself stays in place and learns new state on
// its next refresh. An already-connected provider app navigates the
// tab to the hub's detail page instead; hub bases that are not plain
// HTTPS (or loopback HTTP) or embed credentials are refused with an
// error and the tab closes. A blocked popup explains itself.
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudOAuthBeginRaw } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { connectedStatus, installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

const OAUTH_APP = stubApp({ appId: "app-oauth-x", slug: "oauthx", name: "OAuth X", authType: "oauth2" });

/** Minimal loopback stand-in for the hub detail page (ephemeral port). */
function startLoopbackHub(): Promise<{ base: string; close: () => Promise<void> }> {
  return new Promise((resolve, reject) => {
    const server = createServer((_req, res) => {
      res.writeHead(200, { "content-type": "text/html" });
      res.end("<html><body>Loopback hub app detail</body></html>");
    });
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address() as AddressInfo;
      resolve({
        base: `http://127.0.0.1:${address.port}`,
        close: () =>
          new Promise<void>((done) => {
            server.close(() => done());
          }),
      });
    });
  });
}

test(
  specTitle(["CLOUD-025"], "provider connect runs in a pre-opened tab to the consent handoff"),
  { tag: specTags(["CLOUD-025"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([OAUTH_APP]) }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    try {
      // Live server handoff: the begin bounces back to return_path with
      // an error when no gateway can mint the launch.
      const live = await cloudOAuthBeginRaw(session.cookie, "app-oauth-x", `/${seed.workspaceSlug}/apps`);
      expect(live.status).toBe(302);
      expect(live.location).toContain(`/${seed.workspaceSlug}/apps`);
      expect(live.location).toContain("error=openhub_reauth_required");

      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["OAuth X"]);
      const popupPromise = driver.page.waitForEvent("popup", { timeout: 30_000 });
      await driver.cloudAppsClickCardAction("OAuth X", "Install");
      const popup = await popupPromise;
      // Pre-opened: placeholder content, synchronously, opener detached.
      expect(await popup.title()).toContain("Connecting");
      expect(await popup.evaluate(() => String(window.opener))).toBe("null");
      // Then pointed at the same-origin begin carrying return_path.
      await expect
        .poll(() => stub.log.filter((entry) => entry.path.includes("/auth/oauth/begin/")).map((entry) => entry.path), {
          timeout: 60_000,
        })
        .toHaveLength(1);
      const begin = stub.log.find((entry) => entry.path.includes("/auth/oauth/begin/"))?.path ?? "";
      expect(begin.startsWith("/api/openhub/apps/app-oauth-x/auth/oauth/begin/")).toBe(true);
      expect(new URL(begin, "http://oracle.invalid").searchParams.get("return_path")).toBe(
        `/${seed.workspaceSlug}/apps`
      );
      // The live server handoff ran in the tab: it bounced back with the
      // error, reported it once, and stripped it.
      await expect.poll(() => popup.url(), { timeout: 60_000 }).toBe(`${popup.url().split("?")[0]}`);
      expect(popup.url()).toBe(`http://localhost:${new URL(popup.url()).port}/${seed.workspaceSlug}/apps`);
      const popupToast = popup.locator('[aria-label="Notifications"]');
      await expect
        .poll(() => popupToast.innerText().catch(() => ""), { timeout: 60_000 })
        .toContain("Connection failed");
      // Pi Dash itself never moved.
      expect(await driver.currentPath()).toBe(`/${seed.workspaceSlug}/apps`);
      // State refresh: the next load re-reads the catalog and renders it.
      const readsBefore = stub.log.filter((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/")).length;
      await driver.page.reload({ waitUntil: "domcontentloaded" });
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["OAuth X"]);
      const readsAfter = stub.log.filter((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/")).length;
      expect(readsAfter).toBeGreaterThan(readsBefore);
      await popup.close().catch(() => undefined);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-025"], "blocked popup explains itself instead of failing silently"),
  { tag: specTags(["CLOUD-025"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([OAUTH_APP]) }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    try {
      await driver.page.addInitScript(() => {
        Object.defineProperty(window, "open", { value: () => null });
      });
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["OAuth X"]);
      await driver.cloudAppsClickCardAction("OAuth X", "Install");
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      const toast = (await driver.lastToast()) ?? "";
      expect(toast).toContain("Installed, but not connected");
      expect(toast.toLowerCase()).toContain("pop-up");
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-031"], "already-connected provider apps open in the app hub"),
  { tag: specTags(["CLOUD-031"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({ appId: "com.linear.linear", slug: "linear", name: "Linear", authType: "oauth2" }),
          ]),
        }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Linear"]);
      const popupPromise = driver.page.waitForEvent("popup", { timeout: 30_000 });
      await driver.cloudAppsClickCardAction("Linear", "Install");
      const popup = await popupPromise;
      await expect.poll(() => popup.url(), { timeout: 60_000 }).toBe("https://hub.airepublic.com/apps/linear");
      await popup.close().catch(() => undefined);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-031"], "unsafe hub bases are refused and the tab closes"),
  { tag: specTags(["CLOUD-031"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      status: () => Promise.resolve({ status: 200, body: connectedStatus("http://hub.example.com") }),
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({ appId: "app-plain-http", slug: "plain-http", name: "Plain Http", authType: "oauth2" }),
          ]),
        }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Plain Http"]);
      const popupPromise = driver.page.waitForEvent("popup", { timeout: 30_000 });
      await driver.cloudAppsClickCardAction("Plain Http", "Install");
      const popup = await popupPromise;
      await expect.poll(() => popup.isClosed(), { timeout: 60_000 }).toBe(true);
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      expect((await driver.lastToast()) ?? "").toContain("Installed, but not connected");
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-031"], "loopback hub bases stay navigable over plain HTTP"),
  { tag: specTags(["CLOUD-031"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    // A real loopback listener: a refused port commits chrome-error and
    // drops the attempted URL, so only a live server proves the
    // navigation end to end.
    const hub = await startLoopbackHub();
    const stub = await installOpenhubStub(driver.page.context(), {
      status: () => Promise.resolve({ status: 200, body: connectedStatus(hub.base) }),
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([stubApp({ appId: "app-loop", slug: "loop", name: "Loop App", authType: "oauth2" })]),
        }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Loop App"]);
      const popupPromise = driver.page.waitForEvent("popup", { timeout: 30_000 });
      await driver.cloudAppsClickCardAction("Loop App", "Install");
      const popup = await popupPromise;
      await expect.poll(() => popup.url(), { timeout: 60_000 }).toBe(`${hub.base}/apps/loop`);
      await expect
        .poll(
          () =>
            popup
              .locator("body")
              .innerText()
              .catch(() => ""),
          { timeout: 60_000 }
        )
        .toContain("Loopback hub app detail");
      await popup.close().catch(() => undefined);
    } finally {
      await hub.close();
      await stub.uninstall();
    }
  }
);
