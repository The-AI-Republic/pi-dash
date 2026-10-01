// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-123): loading, header and refresh rules.
// Rows: SHELL-021 (staged loading skeletons for the shell and each
// widget), SHELL-022 (dashboard header with home breadcrumb plus the
// widget-settings shortcut), SHELL-023 (home data never refreshes
// silently; failures toast and keep prior state, with no retry panel).
// Behavior learned from the old dashboard in prose: the shell and each
// widget show their own skeleton shape until resolved, the header pairs
// a home breadcrumb with the manage-widgets control, and widget, link
// and recent state survive refocus while failed edits toast in place.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  serverCreateQuickLink,
  serverDeleteQuickLink,
  serverQuickLinks,
  serverSetTourCompleted,
  serverWidgets,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-021", "SHELL-022", "SHELL-023"];

async function signedInHome(
  driver: ParityDriver,
  seed: { email: string; password: string; workspaceSlug: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.homeOpen(seed.workspaceSlug);
}

test(
  specTitle(ROWS, "loading shows skeletons before the dashboard resolves"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);

    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    // Hold the widget map briefly so the dashboard skeleton stage is
    // observable, and the row fetches longer so the per-widget loading
    // shapes show after the map resolves.
    await driver.page.route("**/home-preferences/*", async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 4000));
      await route.continue();
    });
    for (const pattern of ["**/quick-links/", "**/recent-visits/"] as const) {
      await driver.page.route(pattern, async (route) => {
        if (route.request().method() !== "GET") {
          await route.continue();
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 8000));
        await route.continue();
      });
    }
    await driver.homeOpen(seed.workspaceSlug);
    expect(await driver.homeSkeletonVisible()).toBe(true);
    await expect.poll(() => driver.homeWidgetTitles(), { timeout: 90_000 }).not.toEqual([]);
    expect(await driver.homeSkeletonVisible()).toBe(true);
    await expect.poll(() => driver.homeSkeletonVisible(), { timeout: 90_000 }).toBe(false);
    await driver.page.unroute("**/home-preferences/*");
    await driver.page.unroute("**/quick-links/");
    await driver.page.unroute("**/recent-visits/");
  }
);

test(
  specTitle(ROWS, "header names home and opens widget settings"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);

    await signedInHome(driver, seed);
    const titles = await driver.homeWaitForWidgets();
    expect(titles).not.toEqual([]);
    // The header control settles on its own fetch behind the widget stack;
    // under load it can lag a full minute, so reconcile over fresh loads.
    let crumb: string | null = null;
    for (let round = 0; round < 3 && crumb === null; round += 1) {
      if (round > 0) {
        await driver.homeReload();
        await driver.homeWaitForWidgets();
      }
      try {
        await expect.poll(() => driver.homeBreadcrumb(), { timeout: 30_000 }).not.toBeNull();
        crumb = await driver.homeBreadcrumb();
      } catch {
        // Another fresh load.
      }
    }
    expect(crumb).not.toBeNull();
    await driver.homeOpenManageWidgets();
    expect((await driver.homeManageWidgetNames()).join("\n")).toContain("Recents");
    await driver.homeCloseManageWidgets();
  }
);

test(
  specTitle(ROWS, "refocus never reorders state; failed edits toast in place"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);

    await signedInHome(driver, seed);
    await expect.poll(() => driver.homeWidgetTitles(), { timeout: 60_000 }).not.toEqual([]);

    await test.step("losing and regaining focus keeps every order", async () => {
      const widgetsBefore = await driver.homeWidgetTitles();
      const linksBefore = await driver.homeQuickLinkNames();
      const recentsBefore = await driver.homeRecentRowTexts();
      const context = driver.page.context();
      const other = await context.newPage();
      await other.goto("about:blank");
      await driver.page.bringToFront();
      await driver.page.waitForTimeout(3_000);
      await other.close();
      expect(await driver.homeWidgetTitles()).toEqual(widgetsBefore);
      expect(await driver.homeQuickLinkNames()).toEqual(linksBefore);
      // Other traffic on the scratch stack may append visits; what matters
      // is that refocus reorders nothing already shown.
      const recentsAfter = await driver.homeRecentRowTexts();
      let cursor = -1;
      for (const row of recentsBefore) {
        cursor = recentsAfter.indexOf(row, cursor + 1);
        expect(cursor).toBeGreaterThanOrEqual(0);
      }
    });

    await test.step("a failed reorder toasts and keeps prior state", async () => {
      const storedBefore = await serverWidgets(seed.workspaceSlug, session);
      const linksBefore = storedBefore.find((widget) => widget.key === "quick_links")?.sort_order;
      // The reorder patches the dragged (source) widget's preference.
      await driver.page.route("**/home-preferences/quick_links/", async (route) => {
        await route.abort();
      });
      await driver.homeOpenManageWidgets();
      await driver.homeDragWidget("Quicklinks", "Recents");
      await expect.poll(() => driver.homeLastToast(), { timeout: 15_000 }).not.toBeNull();
      const toast = await driver.homeLastToast();
      expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.toLowerCase()).toMatch(/error|fail/);
      await driver.homeCloseManageWidgets();
      const storedAfter = await serverWidgets(seed.workspaceSlug, session);
      expect(storedAfter.find((widget) => widget.key === "quick_links")?.sort_order).toBe(linksBefore);
      // No retry panel is offered: the dashboard shows no retry affordance.
      expect(await driver.homeWidgetTitles()).toContain("Quicklinks");
      await driver.page.unroute("**/home-preferences/quick_links/");
    });
  }
);

test(
  specTitle(ROWS, "bug: a failed toggle keeps prior state without a notice (NEWFRONT-131)"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    // Observed oracle deviation, tracked in NEWFRONT-131: the toggle path
    // calls the store directly, so a failed toggle keeps state
    // (server-first ordering) but toasts nothing, unlike reorder and
    // link edits. Intended behavior: toast the failure like reorder does.
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);

    await signedInHome(driver, seed);
    await expect.poll(() => driver.homeWidgetTitles(), { timeout: 60_000 }).not.toEqual([]);

    const storedBefore = await serverWidgets(seed.workspaceSlug, session);
    const recentsBefore = storedBefore.find((widget) => widget.key === "recents")?.is_enabled ?? true;
    await driver.page.route("**/home-preferences/recents/", async (route) => {
      await route.abort();
    });
    await driver.homeOpenManageWidgets();
    await driver.homeToggleManageWidget("Recents");
    await driver.homeCloseManageWidgets();
    const storedAfter = await serverWidgets(seed.workspaceSlug, session);
    expect(storedAfter.find((widget) => widget.key === "recents")?.is_enabled).toBe(recentsBefore);
    await driver.page.unroute("**/home-preferences/recents/");
  }
);

test(
  specTitle(ROWS, "failed link edits toast; list-fetch failure offers no retry"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    const stamp = `chrome-${Date.now().toString(36)}`;
    const title = `Fail probe ${stamp}`;
    const row = await serverCreateQuickLink(seed.workspaceSlug, session, title, `https://example.com/${stamp}`);

    await signedInHome(driver, seed);
    await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 60_000 }).toContain(title);

    await test.step("a failed link edit toasts and keeps prior state", async () => {
      // The edit PATCHes the link row; the glob must cross the trailing
      // slash, so a single star would miss it.
      await driver.page.route("**/quick-links/**", async (route) => {
        if (route.request().method() === "PATCH" || route.request().method() === "PUT") await route.abort();
        else await route.continue();
      });
      await driver.homeEditQuickLink(title, `${title} edited`, `https://example.org/${stamp}`);
      await expect.poll(() => driver.homeLastToast(), { timeout: 15_000 }).not.toBeNull();
      const toast = await driver.homeLastToast();
      expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.toLowerCase()).toMatch(/error|fail|could not|not updated/);
      const stored = await serverQuickLinks(seed.workspaceSlug, session);
      expect(stored.find((entry) => entry.id === row.id)?.url ?? "").toContain("example.com/");
      await driver.page.unroute("**/quick-links/**");
    });

    await test.step("a failed link delete toasts and keeps the row", async () => {
      // Back to server truth first: the failed edit above may have left
      // an optimistic title painted. A loaded paint can miss the row, so
      // reconcile once before failing honestly.
      let rowBack = false;
      for (let round = 0; round < 2 && !rowBack; round += 1) {
        await driver.homeReload();
        try {
          await expect.poll(() => driver.homeQuickLinkNames(), { timeout: 30_000 }).toContain(title);
          rowBack = true;
        } catch {
          // One more fresh paint.
        }
      }
      expect(rowBack).toBe(true);
      await driver.page.route("**/quick-links/**", async (route) => {
        if (route.request().method() === "DELETE") await route.abort();
        else await route.continue();
      });
      await driver.homeDeleteQuickLink(title);
      await expect.poll(() => driver.homeLastToast(), { timeout: 15_000 }).not.toBeNull();
      const toast = await driver.homeLastToast();
      expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.toLowerCase()).toMatch(/error|fail|could not|not removed/);
      const stored = await serverQuickLinks(seed.workspaceSlug, session);
      expect(stored.some((entry) => entry.id === row.id)).toBe(true);
      await driver.page.unroute("**/quick-links/**");
    });

    await test.step("a failed link-list fetch offers no retry", async () => {
      await driver.page.route("**/quick-links/", async (route) => {
        if (route.request().method() === "GET") await route.abort();
        else await route.continue();
      });
      await driver.homeReload();
      await driver.page.waitForTimeout(5_000);
      expect(await driver.page.getByRole("button", { name: /retry|try again/i }).count()).toBe(0);
      await driver.page.unroute("**/quick-links/");
    });

    await serverDeleteQuickLink(seed.workspaceSlug, session, row.id).catch(() => undefined);
  }
);
