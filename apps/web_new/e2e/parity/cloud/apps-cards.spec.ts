// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): card states, rows CLOUD-028 (cards
// communicate install and credential state at a glance; icons prefer
// the self-hosted graphic with a proxied fallback and a letter tile;
// per-card busy states guard double clicks), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): installed cards carry an
// installed marker, installed-but-credentialless ones an unfinished
// marker, and uninstalled ones none; actions follow suit (Install /
// Connect+Uninstall / Uninstall); icons prefer inline self-hosted
// graphics, then the proxied icon URL, then the always-rendered
// letter tile when both fail; while one card works its action locks
// and a second click cannot double-post, while other cards stay live.
// Live, the icon proxy rejects misshaped URLs before fetching.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudIconRaw } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

const TINY_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32"/></svg>';

test(
  specTitle(["CLOUD-028"], "cards show install state, actions, and icon sources at a glance"),
  { tag: specTags(["CLOUD-028"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({ appId: "app-fresh", slug: "fresh", name: "Fresh App" }),
            stubApp({
              appId: "app-pending",
              slug: "pending",
              name: "Pending App",
              authType: "api_key",
              isInstalled: true,
              suggestedAction: "connect_auth",
            }),
            stubApp({
              appId: "app-ready",
              slug: "ready",
              name: "Ready App",
              isInstalled: true,
              suggestedAction: "none",
            }),
            stubApp({
              appId: "app-inline",
              slug: "inline",
              name: "Inline Icon",
              iconSvgFallback: TINY_SVG,
              iconUrl: "https://cdn.example.com/ignored.png",
            }),
            stubApp({
              appId: "app-proxied",
              slug: "proxied",
              name: "Proxied Icon",
              iconUrl: "https://cdn.example.com/icon.png",
            }),
          ]),
        }),
      icon: () => Promise.resolve({ status: 200, body: TINY_SVG, contentType: "image/svg+xml" }),
    });
    try {
      // Live server state: the proxy rejects misshaped icon URLs without
      // fetching (no gateway involved).
      for (const bad of ["http://cdn.example.com/icon.png", "not-a-url", ""]) {
        const answer = await cloudIconRaw(session.cookie, bad);
        expect(answer.status).toBe(400);
      }

      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 })
        .toEqual(["Fresh App", "Pending App", "Ready App", "Inline Icon", "Proxied Icon"]);
      await expect
        .poll(() => driver.cloudAppsCards(), { timeout: 60_000 })
        .toEqual([
          { name: "Fresh App", badge: null, actions: ["Install"], icon: "letter", letter: "F" },
          {
            name: "Pending App",
            badge: "needs-connection",
            actions: ["Connect", "Uninstall"],
            icon: "letter",
            letter: "P",
          },
          { name: "Ready App", badge: "installed", actions: ["Uninstall"], icon: "letter", letter: "R" },
          { name: "Inline Icon", badge: null, actions: ["Install"], icon: "inline", letter: "I" },
          { name: "Proxied Icon", badge: null, actions: ["Install"], icon: "proxied", letter: "P" },
        ]);
      // The proxied icon went through the proxy, not straight to the CDN.
      expect(stub.log.some((entry) => entry.path.startsWith("/api/openhub/apps/icon/"))).toBe(true);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-028"], "broken icons fall back to the letter tile"),
  { tag: specTags(["CLOUD-028"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({
              appId: "app-broken",
              slug: "broken",
              name: "Broken Icon",
              iconUrl: "https://cdn.example.com/missing.png",
            }),
          ]),
        }),
      icon: () => Promise.resolve({ status: 404, body: {}, contentType: "application/json" }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.cloudAppsCards(), { timeout: 60_000 })
        .toEqual([{ name: "Broken Icon", badge: null, actions: ["Install"], icon: "letter", letter: "B" }]);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-028"], "per-card busy state guards double clicks"),
  { tag: specTags(["CLOUD-028"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    let posts = 0;
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () =>
        Promise.resolve({
          status: 200,
          body: marketplacePage([
            stubApp({ appId: "app-slow", slug: "slow", name: "Slow App" }),
            stubApp({ appId: "app-other", slug: "other", name: "Other App" }),
          ]),
        }),
      install: async () => {
        posts += 1;
        await gate;
        return { status: 200, body: { installed: true } };
      },
      authStatus: () => Promise.resolve({ status: 200, body: { type: "none", status: "active" } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Slow App", "Other App"]);
      await driver.cloudAppsClickCardAction("Slow App", "Install");
      // The working card locks while its neighbor stays live.
      await expect
        .poll(() => driver.cloudAppsCardActionDisabled("Slow App", "Install"), {
          timeout: 60_000,
        })
        .toBe(true);
      expect(await driver.cloudAppsCardActionDisabled("Other App", "Install")).toBe(false);
      release();
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      expect(posts).toBe(1);
      expect(await driver.cloudAppsCardActionDisabled("Slow App", "Install")).toBe(false);
    } finally {
      await stub.uninstall();
    }
  }
);
