// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): install-then-connect, row CLOUD-024
// (install commits first and refreshes the list before the connect step
// starts; success, partial, and failure outcomes each report
// distinctly), green on the cloud-edition old app first. The same file
// must go green on apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): Install posts, then re-reads
// the catalog before asking for auth status, so a later failure reads
// as not-connected rather than not-installed; a clean connect toasts
// success, a failed commit toasts install-failed with the server
// detail and never starts connecting, and a commit whose connect step
// throws toasts installed-but-unconnected. Live (no gateway), the
// server refuses installs with the reauth taxonomy.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudInstallRaw } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp, connectedStatus } from "./openhub-stub";

test(
  specTitle(["CLOUD-024"], "install commits and refreshes before connecting"),
  { tag: specTags(["CLOUD-024"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let installed = false;
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({
              appId: "app-plain",
              slug: "plain",
              name: "Plain App",
              isInstalled: installed,
              suggestedAction: installed ? "none" : "install",
            }),
          ]),
        }),
      install: () => {
        installed = true;
        return Promise.resolve({ status: 200, body: { installed: true } });
      },
      authStatus: () => Promise.resolve({ status: 200, body: { type: "none", status: "active" } }),
    });
    try {
      // Live server state behind scratch: no gateway, so the proxy
      // refuses with the reauth taxonomy (the browser never sees this —
      // the app bounces live 401s to sign-in, hence the stub above).
      const live = await cloudInstallRaw(session.cookie, "app-plain");
      expect(live.status).toBe(401);
      expect(live.payload["error"]).toBe("openhub_reauth_required");

      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Plain App"]);
      await driver.cloudAppsClickCardAction("Plain App", "Install");
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      expect((await driver.lastToast()) ?? "").toContain("Installed");

      // Commit first, list refresh second, connect third.
      const installIdx = stub.log.findIndex((entry) => entry.method === "POST" && entry.path.endsWith("/install/"));
      const refreshIdx = stub.log.findIndex(
        (entry, index) => index > installIdx && entry.path.startsWith("/api/openhub/apps/marketplace/")
      );
      const authIdx = stub.log.findIndex((entry) => entry.path.endsWith("/auth/status/"));
      expect(installIdx).toBeGreaterThanOrEqual(0);
      expect(refreshIdx).toBeGreaterThan(installIdx);
      expect(authIdx).toBeGreaterThan(refreshIdx);
      // The refresh rendered: the card now reads installed.
      await expect
        .poll(() => driver.cloudAppsCards(), { timeout: 60_000 })
        .toEqual([expect.objectContaining({ name: "Plain App", badge: "installed", actions: ["Uninstall"] })]);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-024"], "failed install reports and never starts connecting"),
  { tag: specTags(["CLOUD-024"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([stubApp({ appId: "app-doomed", slug: "doomed", name: "Doomed App" })]),
        }),
      install: () =>
        Promise.resolve({ status: 502, body: { error: "openhub_error", detail: "Hub refused the install." } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Doomed App"]);
      await driver.cloudAppsClickCardAction("Doomed App", "Install");
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      const toast = (await driver.lastToast()) ?? "";
      expect(toast).toContain("Install failed");
      expect(toast).toContain("Hub refused the install.");
      expect(stub.log.some((entry) => entry.path.endsWith("/auth/status/"))).toBe(false);
      expect(await driver.cloudAppsCards()).toEqual([
        expect.objectContaining({ name: "Doomed App", badge: null, actions: ["Install"] }),
      ]);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-024"], "installed-but-unconnected reports distinctly and closes its tab"),
  { tag: specTags(["CLOUD-024"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      // A hub base with embedded credentials: the already-connected
      // branch refuses to navigate there and throws instead.
      status: () => Promise.resolve({ status: 200, body: connectedStatus("https://user:pass@hub.example.com") }),
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({ appId: "app-oauth-c", slug: "oauth-c", name: "OAuth Capped", authType: "oauth2" }),
          ]),
        }),
      install: () => Promise.resolve({ status: 200, body: { installed: true } }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["OAuth Capped"]);
      const popupPromise = driver.page.waitForEvent("popup", { timeout: 30_000 });
      await driver.cloudAppsClickCardAction("OAuth Capped", "Install");
      const popup = await popupPromise;
      await expect.poll(() => popup.isClosed(), { timeout: 60_000 }).toBe(true);
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      const toast = (await driver.lastToast()) ?? "";
      expect(toast).toContain("Installed, but not connected");
      // The commit still refreshed the list before the connect threw.
      const installIdx = stub.log.findIndex((entry) => entry.method === "POST" && entry.path.endsWith("/install/"));
      const refreshIdx = stub.log.findIndex(
        (entry, index) => index > installIdx && entry.path.startsWith("/api/openhub/apps/marketplace/")
      );
      expect(installIdx).toBeGreaterThanOrEqual(0);
      expect(refreshIdx).toBeGreaterThan(installIdx);
    } finally {
      await stub.uninstall();
    }
  }
);
