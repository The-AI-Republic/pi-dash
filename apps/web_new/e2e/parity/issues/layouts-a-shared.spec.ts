// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-117): shared layout behavior — switching the
// issue layout through the header control (ISS-001) and the per-user
// per-entity preference persistence behind it (ISS-002).
import { test, expect } from "../fixtures";
import {
  seedProjectUserProperties,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// The desktop header (segmented layout control) only renders above the
// content-area breakpoint; the default 1280px viewport shows the compact
// dropdown instead, which the mobile spec covers separately.
test.use({ viewport: { width: 1600, height: 900 } });

const ROWS_001 = ["ISS-001"];
const ROWS_002 = ["ISS-002"];

async function resetPrefs(workspaceSlug: string, projectId: string, session: string): Promise<void> {
  const seed = seedProjectUserProperties();
  await serverPatchProjectUserProperties(workspaceSlug, projectId, session, {
    display_filters: seed.display_filters,
    display_properties: seed.display_properties,
  });
}

test(
  specTitle(ROWS_001, "switch the issue layout through the header control"),
  { tag: specTags(ROWS_001) },
  async ({ driver, seed }) => {
    // Five switches including the slow first gantt compile (~150s loaded).
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );

    await test.step("all five layouts are offered, list is active", async () => {
      expect(await driver.layoutsOfferedLayouts()).toEqual(["list", "kanban", "calendar", "spreadsheet", "gantt"]);
      expect(await driver.layoutsActiveLayout()).toEqual("list");
      await expect.poll(async () => driver.layoutsListVisible(), { timeout: 120_000 }).toEqual(true);
    });

    const switches = [
      { layout: "calendar", visible: () => driver.layoutsCalendarVisible() },
      { layout: "spreadsheet", visible: () => driver.layoutsSpreadsheetVisible() },
      { layout: "kanban", visible: () => driver.layoutsKanbanVisible() },
      { layout: "gantt", visible: () => driver.layoutsGanttVisible() },
    ] as const;
    for (const { layout, visible } of switches) {
      await test.step(`switch to ${layout} renders it and persists the choice`, async () => {
        await driver.layoutsSwitchTo(layout);
        expect(await driver.layoutsActiveLayout()).toEqual(layout);
        expect(await visible()).toEqual(true);
        await expect
          .poll(
            async () =>
              (await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session)).displayFilters["layout"],
            { timeout: 30_000 }
          )
          .toEqual(layout);
      });
    }

    await test.step("back to list; clicking the active layout is a no-op", async () => {
      await driver.layoutsSwitchTo("list");
      expect(await driver.layoutsActiveLayout()).toEqual("list");
      expect(await driver.layoutsListVisible()).toEqual(true);
      await driver.layoutsSwitchTo("list");
      expect(await driver.layoutsActiveLayout()).toEqual("list");
      expect(await driver.layoutsListVisible()).toEqual(true);
    });

    await test.step("teardown restores the seeded preferences", async () => {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    });
  }
);

test(specTitle(ROWS_001, "guests can switch layouts too"), { tag: specTags(ROWS_001) }, async ({ driver, seed }) => {
  test.setTimeout(720_000);
  if (!seed.guestEmail || !seed.guestPassword) {
    throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
  }
  const session = await signInSession(seed.guestEmail, seed.guestPassword);
  await resetPrefs(seed.workspaceSlug, seed.projectId, session);
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
    sessionBrowserCookies(session)
  );

  await driver.layoutsSwitchTo("calendar");
  expect(await driver.layoutsActiveLayout()).toEqual("calendar");
  expect(await driver.layoutsCalendarVisible()).toEqual(true);

  await resetPrefs(seed.workspaceSlug, seed.projectId, session);
});

test(
  specTitle(ROWS_002, "layout preferences persist per user and survive reload"),
  { tag: specTags(ROWS_002) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const session = await signInSession(seed.email, seed.password);
    await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      sessionBrowserCookies(session)
    );

    await test.step("a UI switch survives reload", async () => {
      await driver.layoutsSwitchTo("spreadsheet");
      await driver.layoutsReloadIssues();
      expect(await driver.layoutsActiveLayout()).toEqual("spreadsheet");
      expect(await driver.layoutsSpreadsheetVisible()).toEqual(true);
      const props = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(props.displayFilters["layout"]).toEqual("spreadsheet");
    });

    await test.step("grouping restores from the stored preferences", async () => {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: "state", order_by: "sort_order" },
      });
      await driver.layoutsReloadIssues();
      await expect.poll(async () => driver.layoutsListVisible(), { timeout: 120_000 }).toEqual(true);
      await expect.poll(async () => driver.layoutsListGroups(), { timeout: 120_000 }).toEqual(["Todo"]);
    });

    await test.step("sort order restores from the stored preferences", async () => {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: null, order_by: "-created_at" },
      });
      await driver.layoutsReloadIssues();
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
        .toEqual([...seed.issueNames].reverse());
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
      });
      await driver.layoutsReloadIssues();
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
        .toEqual([...seed.issueNames]);
    });

    await test.step("display properties restore from the stored preferences", async () => {
      await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, session, {
        display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
        display_properties: { state: false },
      });
      await driver.layoutsReloadIssues();
      await expect
        .poll(async () => driver.layoutsListGroupIssueNames("All work items"), { timeout: 120_000 })
        .toEqual([...seed.issueNames]);
      await expect.poll(async () => driver.hasVisibleText("Todo"), { timeout: 120_000 }).toEqual(false);
    });

    await test.step("teardown restores the seeded preferences", async () => {
      await resetPrefs(seed.workspaceSlug, seed.projectId, session);
    });
  }
);
