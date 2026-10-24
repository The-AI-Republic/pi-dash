// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for the cloud frame, edition marker, upgrade pill and
// desktop update control (NEWFRONT-126). The seeded build suppresses the
// rail everywhere, never mounts the edition badge, and ships a web update
// control that renders nothing; the upgrade pill still marks the paywalled
// Cycles destination in its page header without touching navigation. Anything
// needing the matching cloud or desktop build is recorded as a gap in the
// inventory. Rows: SHELL-099 (suppression half), SHELL-100, SHELL-101,
// SHELL-102.
import { desktopInvocations, installDesktopMock } from "../../desktop/tauri-mock";
import { test, expect } from "../../fixtures";
import { serverSetTourCompleted, sessionBrowserCookies, sessionValid, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-099", "SHELL-100", "SHELL-102"];
const PILL = ["SHELL-101"];
const POSITIVE = ["SHELL-102"];

/** Release the mock shell holds in the desktop-positive leg; plainly fake versions. */
const WAITING_UPDATE = { version: "9.9.9", currentVersion: "9.9.8", installing: false };

/** Entry heading the desktop bundle renders for a signed-out visitor (DESK-011). Absent on web. */
const DESKTOP_ENTRY_HEADING = "The desktop app can't sign you in yet";

test(
  specTitle(ROWS, "cloud frame, edition marker, desktop update"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    const session = await signInSession(seed.email, seed.password);
    await serverSetTourCompleted(session, true);

    await test.step("rail-suppressed pages keep full content width", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      expect(await driver.railPresent()).toBe(false);
      await driver.openProjectTab(seed.workspaceSlug, seed.projectId, "issues");
      expect(await driver.railPresent()).toBe(false);
    });

    await test.step("no edition badge mounts in this build", async () => {
      expect(await driver.editionBadgePresent()).toBe(false);
    });

    await test.step("no desktop update control renders in the web sidebar", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      expect(await driver.desktopUpdatePresent()).toBe(false);
    });
  }
);

test(
  specTitle(PILL, "upgrade pills mark destinations without affecting navigation"),
  { tag: specTags(PILL) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });

    // The first-run tour overlay covers the dashboard for fresh users, so clear it.
    const session = await signInSession(seed.email, seed.password);
    await serverSetTourCompleted(session, true);

    // The pill mounts beside the paywalled Cycles header (the sidebar rows
    // that carry the small variant are dead or owned by other areas in this
    // build), so prove the header mount and that the page still navigates.
    await driver.page.goto(`/${seed.workspaceSlug}/active-cycles/`);
    await driver.page.waitForLoadState("domcontentloaded");
    await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain("/active-cycles");
    await expect.poll(() => driver.activeCyclesHeaderVisible(), { timeout: 30_000 }).toBe(true);
    await expect.poll(() => driver.upgradePillCount(), { timeout: 15_000 }).toBeGreaterThan(0);
    await driver.openWorkspaceHome(seed.workspaceSlug);
    await expect.poll(() => driver.page.url(), { timeout: 30_000 }).toContain(`/${seed.workspaceSlug}/`);
  }
);

test(
  specTitle(POSITIVE, "desktop builds render the update control in the sidebar"),
  { tag: specTags(POSITIVE) },
  async ({ driver, seed }) => {
    // Desktop-positive leg (NEWFRONT-527): the web-negative half lives
    // in the first test above. The oracle project drives the old web
    // build only, and the web bundle carries no updater capability, so
    // this leg runs solely against the desktop bundle with the mocked
    // shell the desktop gate staged (desktop/updater/updater.spec.ts
    // proves the control's own lifecycle on the same rig).
    if (driver.target === "web") {
      test.skip(true, "the oracle project drives the old web build only; no desktop bundle to prove");
      return;
    }

    // The entry screen differs by build target: the desktop bundle
    // renders its own sign-in screen while the web bundle walks the
    // email step. Probe before installing the mock bridge — the init
    // script must land before the first mock-backed navigation.
    await driver.visit("/");
    let bundle: "desktop" | "web" | "pending" = "pending";
    await expect
      .poll(
        async () => {
          if ((await driver.page.getByRole("heading", { name: DESKTOP_ENTRY_HEADING }).count()) > 0) {
            bundle = "desktop";
          } else if ((await driver.authStep()) !== "unknown") {
            bundle = "web";
          }
          return bundle;
        },
        { timeout: 60_000 }
      )
      .not.toBe("pending");
    test.skip(bundle !== "desktop", "desktop-positive leg needs the desktop bundle (PIDASH_TARGET=desktop)");

    await test.step("a waiting update mounts the control in the sidebar", async () => {
      await installDesktopMock(driver.page, { pendingUpdate: WAITING_UPDATE });
      const session = await signInSession(seed.email, seed.password);
      await serverSetTourCompleted(session, true);
      await driver.page.context().addCookies(sessionBrowserCookies(session));
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      // The shell owns the mount point; the control's install/retry
      // behavior is DESK-018's, proven in the desktop suite.
      await expect.poll(() => driver.desktopUpdatePresent(), { timeout: 30_000 }).toBe(true);
      const pending = await desktopInvocations(driver.page, "desktop_pending_update");
      expect(pending.length).toBeGreaterThanOrEqual(1);
    });

    await test.step("the server session stays valid throughout", async () => {
      expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
    });
  }
);
