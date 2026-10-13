// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-209): desktop updater web-observable sides.
// Rows: DESK-016–DESK-018.
//
// The oracle runs on apps/web, where the updater is absent by design: the
// launch prompt and the daily check live in the native shell (updates.rs),
// and the install button lives in the desktop bundle (the web-build stub
// renders null beside the sidebar user menu). Each scenario pins the
// web-observable side: launching and using the web app shows no update
// dialog, a web session emits no update-feed traffic, and the sidebar
// carries no update button.
//
// No update feed endpoint exists in this repo to probe: tauri.conf.json
// ships no plugins.updater section, and main.rs enables the updater only
// when a distributor supplies the key plus endpoints through a --config
// overlay — otherwise the plugin is neither registered nor polled. The
// DESK-017 watch therefore asserts that no feed-shaped request (release
// manifest paths, updater endpoints) leaves the page during a full signed-in
// session, with non-vacuity legs proving the watch saw real traffic. The
// suite never opens the "What's new?" product-updates dialog (DESK-015 /
// SHELL-060 territory), so its changelog iframe cannot pollute the watch.
//
// Overlap: DESK-015 owns the updates dialog and its externals, AUTH-020
// owns the signed-out entry bounce, and the overlay button suite owns the
// button's desktop behavior (accessible name, installing state, retry).
// These scenarios assert only the DESK seam angles: prompt absence across
// entry and signed-in use, feed-traffic silence with a working watch, and
// button absence across sidebar mounts with no native bridge behind it.
//
// Fixtures: the seeded workspace for the signed-in legs; no extra accounts
// or seed changes needed.
import { test, expect } from "../../fixtures";
import { sessionValid, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

/** Title of the native launch prompt (updates.rs). Absent on web. */
const NATIVE_PROMPT_BODY = "Install now? The app will restart automatically.";

/** Title of the native install-failure dialog (updates.rs). Absent on web. */
const NATIVE_FAILURE_TITLE = "Pi Dash update failed";

/** Accessible-name prefix of the desktop install button. Absent on web. */
const INSTALL_BUTTON_NAME = "Update to Pi Dash ";

/** Tooltip the desktop install button shows while downloading. Absent on web. */
const INSTALLING_TOOLTIP = "Installing update…";

const POLL_60 = { timeout: 60_000 };

/** Substrings that mark a request as update-feed traffic. */
const FEED_MARKERS = ["latest.json", "/releases/", "updater", "update-check", "update_manifest"];

/** Request URLs of the watch that look like update-feed traffic. */
function feedTraffic(urls: string[]): string[] {
  return urls.filter((url) => FEED_MARKERS.some((marker) => url.includes(marker)));
}

test(
  specTitle(["DESK-016"], "launching and using the web app shows no update dialog"),
  { tag: specTags(["DESK-016"]) },
  async ({ driver, seed }) => {
    await test.step("entry shows the sign-in with no update prompt", async () => {
      await driver.visit("/");
      await expect.poll(() => driver.authStep(), POLL_60).toBe("email");
      expect(await driver.deskUpdateNativePromptVisible()).toBe(false);
      const text = await driver.pageText();
      expect(text).not.toContain(NATIVE_PROMPT_BODY);
      expect(text).not.toContain(NATIVE_FAILURE_TITLE);
    });

    await test.step("signed-in use stays prompt-free with no native bridge", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      expect(await driver.deskUpdateNativePromptVisible()).toBe(false);
      const text = await driver.pageText();
      expect(text).not.toContain(NATIVE_PROMPT_BODY);
      expect(text).not.toContain(NATIVE_FAILURE_TITLE);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-017"], "a web session emits no update-feed traffic"),
  { tag: specTags(["DESK-017"]) },
  async ({ driver, seed }) => {
    await driver.deskUpdateBeginTrafficWatch();
    try {
      await test.step("sign in and work through a full session", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
        await driver.openWorkspaceHome(seed.workspaceSlug);
        await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
        await driver.visit(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
        await expect.poll(() => driver.currentPath(), POLL_60).toContain("/issues");
        // The desktop checks on a schedule of its own; sit through a window
        // so a polling web side would have had time to speak.
        await driver.page.waitForTimeout(5_000);
      });

      await test.step("the watch saw the session but no feed traffic", async () => {
        const urls = await driver.deskUpdateTrafficUrls();
        // Non-vacuity: the watch observed the session's real traffic.
        expect(urls.length).toBeGreaterThan(0);
        expect(urls.some((url) => url.includes("/api/"))).toBe(true);
        expect(feedTraffic(urls)).toEqual([]);
      });

      await test.step("no native bridge exists to check from", async () => {
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });
    } finally {
      await driver.deskUpdateEndTrafficWatch().catch(() => undefined);
    }

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);

test(
  specTitle(["DESK-018"], "the sidebar carries no update button on web"),
  { tag: specTags(["DESK-018"]) },
  async ({ driver, seed }) => {
    await test.step("sign in and reach the workspace", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), POLL_60).toContain("Home");
    });

    await test.step("no install button beside the user menu", async () => {
      // On desktop the button mounts here once an update waits (found by
      // the daily check or deferred at launch); the web-build stub renders
      // nothing, so its accessible name must never appear.
      expect(await driver.deskUpdateSidebarButtonVisible()).toBe(false);
      const text = await driver.pageText();
      expect(text).not.toContain(INSTALL_BUTTON_NAME);
      expect(text).not.toContain(INSTALLING_TOOLTIP);
    });

    await test.step("navigating keeps it absent across sidebar mounts", async () => {
      // The sidebar remounts across routes; the desktop button re-queries
      // its pending update on every mount, so re-check after one.
      await driver.visit(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`);
      await expect.poll(() => driver.currentPath(), POLL_60).toContain("/issues");
      expect(await driver.deskUpdateSidebarButtonVisible()).toBe(false);
      expect(await driver.pageText()).not.toContain(INSTALL_BUTTON_NAME);
    });

    await test.step("no native bridge backs the pending-update commands", async () => {
      // desktop_pending_update / desktop_install_update are Tauri commands;
      // without the bridge there is nothing to ask or invoke.
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);
