// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-117): mobile layouts — the compact header
// (ISS-003), the calendar day-detail (ISS-027), and the mobile clauses of
// the calendar options (ISS-023) and drag (ISS-024) rows. Runs on chromium
// with a phone viewport plus touch and a mobile user agent (no device
// descriptor: the suite is chromium-only): the compact header is
// viewport-driven while the touch behavior keys off the mobile UA.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  seedProjectUserProperties,
  serverCreateIssue,
  serverDeleteIssue,
  serverIssueDetails,
  serverPatchIssue,
  serverPatchProject,
  serverPatchProjectUserProperties,
  serverProjectDetails,
  serverProjectUserProperties,
  sessionBrowserCookies,
  signInSession,
  uniqueSuffix,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
  userAgent:
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
});

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

async function openIssues(
  driver: Pick<ParityDriver, "openAuthenticated">,
  seed: Pick<ParitySeedFacts, "workspaceSlug" | "projectId">,
  session: string,
  layout = "list"
): Promise<void> {
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
    display_filters: { layout, group_by: null, order_by: "sort_order" },
  });
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
    sessionBrowserCookies(session)
  );
}

test(
  specTitle(["ISS-003"], "compact header offers layouts, display, and analytics"),
  { tag: specTags(["ISS-003"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    try {
      await openIssues(driver, seed, session, "list");
      await expect
        .poll(async () => driver.layoutsMobileOfferedLayouts(), { timeout: 300_000 })
        .toEqual(["list", "kanban", "calendar"]);
      expect(await driver.layoutsMobileDisplayVisible()).toEqual(true);
      expect(await driver.layoutsMobileAnalyticsVisible()).toEqual(true);

      await driver.layoutsMobileSwitchTo("calendar");
      expect(await driver.layoutsActiveLayout()).toEqual("calendar");
      expect(await driver.layoutsCalendarVisible()).toEqual(true);
      expect(
        (await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).displayFilters["layout"]
      ).toEqual("calendar");

      await driver.layoutsMobileSwitchTo("list");
      expect(await driver.layoutsActiveLayout()).toEqual("list");
    } finally {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-003"], "cycle/module display options disable with their features off"),
  { tag: specTags(["ISS-003"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    // The seed ships both views off; the test needs them on first, and
    // restores the originals afterwards.
    const flags = await serverProjectDetails(seed.workspaceSlug, seed.projectId, session);
    await serverPatchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: true, module_view: true });
    try {
      await openIssues(driver, seed, session, "list");
      await expect
        .poll(async () => driver.layoutsMobileOfferedLayouts(), { timeout: 300_000 })
        .toEqual(["list", "kanban", "calendar"]);
      expect(await driver.layoutsMobileDisplayCycleModuleDisabled()).toEqual({
        cycleDisabled: false,
        moduleDisabled: false,
      });

      await serverPatchProject(seed.workspaceSlug, seed.projectId, session, { cycle_view: false, module_view: false });
      await driver.layoutsReloadIssues();
      await expect
        .poll(async () => driver.layoutsMobileDisplayCycleModuleDisabled(), { timeout: 300_000 })
        .toEqual({ cycleDisabled: true, moduleDisabled: true });
    } finally {
      await serverPatchProject(seed.workspaceSlug, seed.projectId, session, {
        cycle_view: flags.cycleView,
        module_view: flags.moduleView,
      });
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-027", "ISS-023"], "tapping a day lists its issues; options stay usable"),
  { tag: specTags(["ISS-027", "ISS-023"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const today = new Date();
    const iso = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
    const name = `Parity mobday ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: iso }, session);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: {
          layout: "calendar",
          group_by: null,
          order_by: "sort_order",
          calendar: { layout: "month", show_weekends: true },
        },
      });
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
        sessionBrowserCookies(session)
      );
      // Switching options on mobile auto-closes the popover: the day grid
      // stays interactive immediately after.
      await driver.layoutsCalSetMode("week");
      expect(await driver.layoutsCalMode()).toEqual("week");
      await driver.layoutsCalSetMode("month");
      expect(await driver.layoutsCalMode()).toEqual("month");

      await driver.layoutsCalTapDay(today.getDate());
      await expect.poll(async () => driver.layoutsCalDayDetailNames(), { timeout: 300_000 }).toContain(name);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);

test(
  specTitle(["ISS-024"], "calendar blocks do not drag on mobile"),
  { tag: specTags(["ISS-024"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    const today = new Date();
    const last = new Date(today.getFullYear(), today.getMonth() + 1, 0).getDate();
    const fromDay = Math.min(Math.max(today.getDate(), 2), last - 2);
    const toDay = fromDay + 1;
    const iso = (day: number): string =>
      `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    const name = `Parity mobdrag ${uniqueSuffix()}`;
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, session, name);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, id, { target_date: iso(fromDay) }, session);
    try {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: {
          layout: "calendar",
          group_by: null,
          order_by: "sort_order",
          calendar: { layout: "month", show_weekends: true },
        },
      });
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
        sessionBrowserCookies(session)
      );
      // Gate on the rendered month grid; mobile tiles carry no block
      // anchors even for dated issues, so there is no draggable source.
      expect(await driver.layoutsCalMode()).toEqual("month");
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session)).targetDate).toEqual(
        iso(fromDay)
      );
      expect(await driver.layoutsCalDayIssueNames(fromDay)).toEqual([]);
      // A tile-press travel to another day moves nothing.
      await driver.layoutsCalTileDrag(fromDay, toDay);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, session)).targetDate).toEqual(
        iso(fromDay)
      );
      expect(await driver.layoutsCalDayIssueNames(toDay)).toEqual([]);
    } finally {
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, session).catch(() => {});
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    }
  }
);
