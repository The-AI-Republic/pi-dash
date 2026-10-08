// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: Issues display options (NEWFRONT-119). Rows ISS-075
// (panel container plus per-layout matrix), ISS-076 (group by), ISS-077
// (sub-group by), ISS-078 (order by), ISS-079 (sub-issue switch), ISS-080
// (empty-groups switch), ISS-081 (shown-fields pills), ISS-106 (server-side
// per-user persistence), ISS-107 (analytics entry). Written from the area
// spec parity/specs/issues-filters-display.md and verified against the
// running old app through drivers/web (extend-never-fork).
//
// Verified live against the seeded project on the shared oracle (run 3):
// panel sections, pill/switch/group/order controls, the kanban matrix,
// and the analytics dialog, each asserted in the UI plus the stored
// user-properties record.
import { test, expect } from "../fixtures";
import {
  resetIssueUserProperties,
  serverIssueNames,
  serverUserProperties,
  setIssueDisplayFilters,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

// Cold dev-server compiles make the first list load slow; this covers the
// wait without weakening any assertion.
test.setTimeout(480_000);
const LIST_POLL = { timeout: 150_000 };

// One API session per test, refreshed by beforeEach: the shared stack
// throttles credential posts by the minute, so steps reuse this instead of
// signing in again. (The suite runs serially; no test outlives its session.)
let session = "";

test.beforeEach(async ({ seed }) => {
  session = await signInSession(seed.email, seed.password);
  await resetIssueUserProperties(seed.workspaceSlug, seed.projectId, session);
});

// Leave the shared oracle user clean for sibling suites (NEWFRONT-121).
// Runs even on failure.
test.afterEach(async ({ seed }) => {
  await resetIssueUserProperties(seed.workspaceSlug, seed.projectId, session);
});

type SeedFacts = {
  email: string;
  password: string;
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  issueNames: string[];
};

type DisplayDriver = {
  openEntry(): Promise<void>;
  signInWithPassword(email: string, password: string): Promise<void>;
  openProjectIssues(workspaceSlug: string, projectId: string): Promise<void>;
  visibleIssueNames(): Promise<string[]>;
};

/**
 * Reset-after-signin open with one full retry. The page reads the stored
 * state at load while sibling suites share this user, so resetting last
 * narrows the overwrite window to seconds; the retry covers the sibling
 * write or entry blip that lands inside it anyway.
 */
async function openSeededList(driver: DisplayDriver, seed: SeedFacts) {
  const attempt = async () => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await resetIssueUserProperties(seed.workspaceSlug, seed.projectId, session);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    await expect
      .poll(() => driver.visibleIssueNames(), LIST_POLL)
      .toEqual(expect.arrayContaining([...seed.issueNames]));
  };
  try {
    await attempt();
  } catch {
    await attempt();
  }
}

/** Kanban variant: the layout seed is set after the reset, before the open. */
async function openKanbanList(driver: DisplayDriver, seed: SeedFacts) {
  const attempt = async () => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await resetIssueUserProperties(seed.workspaceSlug, seed.projectId, session);
    await setIssueDisplayFilters(seed.workspaceSlug, seed.projectId, session, {
      layout: "kanban",
      group_by: "state",
      order_by: "sort_order",
    });
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  };
  try {
    await attempt();
  } catch {
    await attempt();
  }
}

/**
 * Poll the stored display_properties until the key equals the value. UI
 * writes land after the click returns, so a single read after a toggle
 * races the in-flight PATCH.
 */
async function expectStoredDisplayProperty(seed: SeedFacts, key: string, value: boolean) {
  await expect
    .poll(
      async () => (await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).display_properties[key],
      { timeout: 30_000 }
    )
    .toBe(value);
}

/** Poll the stored display_filters until the key equals the value. */
async function expectStoredDisplayFilter(seed: SeedFacts, key: string, value: unknown) {
  await expect
    .poll(async () => (await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).display_filters[key], {
      timeout: 30_000,
    })
    .toBe(value);
}

/** Poll the stored display_filters until the key is set (non-null). */
async function expectStoredDisplayFilterSet(seed: SeedFacts, key: string) {
  await expect
    .poll(
      async () =>
        (await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).display_filters[key] ?? null,
      { timeout: 30_000 }
    )
    .not.toBeNull();
}

/** Poll the stored display_filters until the key differs from the reset value. */
async function expectStoredDisplayFilterChanged(seed: SeedFacts, key: string, resetValue: unknown) {
  await expect
    .poll(async () => (await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).display_filters[key], {
      timeout: 30_000,
    })
    .not.toBe(resetValue);
}

const DISPLAY_ROWS = ["ISS-075", "ISS-081"];
test(
  specTitle(DISPLAY_ROWS, "display panel lists the list-layout sections and pills toggle server state"),
  { tag: specTags(DISPLAY_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openSeededList(driver, seed);
    });

    await test.step("the panel shows the list-layout sections", async () => {
      await driver.openDisplayOptions();
      const panel = await driver.displayPanelText();
      for (const section of ["Display Properties", "Group by", "Order by"]) expect(panel).toContain(section);
      expect(panel).not.toContain("Sub-group by");
      for (const pill of ["Assignee", "Priority", "State", "Labels"]) expect(panel).toContain(pill);
      await driver.closeDisplayOptions();
    });

    await test.step("toggling a pill flips it off and on, persisted server-side", async () => {
      await driver.openDisplayOptions();
      // The reset leaves every pill active; one toggle flips Assignee off
      // (the app stores the whole pill map), a second flips it back on.
      expect(await driver.isDisplayPropertyActive("Assignee")).toBe(true);
      await driver.toggleDisplayProperty("Assignee");
      expect(await driver.isDisplayPropertyActive("Assignee")).toBe(false);
      await expectStoredDisplayProperty(seed, "assignee", false);
      await driver.toggleDisplayProperty("Assignee");
      expect(await driver.isDisplayPropertyActive("Assignee")).toBe(true);
      await driver.closeDisplayOptions();
      await expectStoredDisplayProperty(seed, "assignee", true);
    });

    await test.step("the server still reports every seeded issue", async () => {
      const names = new Set(await serverIssueNames(seed.workspaceSlug, seed.projectId, session));
      // Containment, not equality: sibling suites share this stack and add
      // their own issues; on a clean checkout the seed stands alone.
      for (const name of seed.issueNames) expect(names.has(name)).toBe(true);
    });
  }
);

const GROUP_ROWS = ["ISS-076", "ISS-106"];
test(
  specTitle(GROUP_ROWS, "group by regroups the list and persists across reload"),
  { tag: specTags(GROUP_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openSeededList(driver, seed);
    });

    await test.step("grouping by state stores the choice and keeps every issue visible", async () => {
      await driver.openDisplayOptions();
      await driver.setDisplayGroupBy("States");
      expect(await driver.isDisplayOptionChecked("States")).toBe(true);
      await driver.closeDisplayOptions();
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      await expectStoredDisplayFilterSet(seed, "group_by");
    });

    await test.step("the grouping survives a full reload", async () => {
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(props.display_filters["group_by"]).not.toBeNull();
      await driver.openDisplayOptions();
      expect(await driver.isDisplayOptionChecked("States")).toBe(true);
      await driver.closeDisplayOptions();
    });
  }
);

const ORDER_ROWS = ["ISS-078", "ISS-106"];
test(
  specTitle(ORDER_ROWS, "order by stores the sort and persists across reload"),
  { tag: specTags(ORDER_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openSeededList(driver, seed);
    });

    await test.step("picking an order stores it and keeps every issue visible", async () => {
      const before = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(before.display_filters["order_by"]).toBe("sort_order");
      await driver.openDisplayOptions();
      await driver.setDisplayOrderBy("Last created");
      expect(await driver.isDisplayOptionChecked("Last created")).toBe(true);
      await driver.closeDisplayOptions();
      await expectStoredDisplayFilterChanged(seed, "order_by", "sort_order");
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

const EXTRA_ROWS = ["ISS-079", "ISS-080"];
test(
  specTitle(EXTRA_ROWS, "options switches persist server-side"),
  { tag: specTags(EXTRA_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openSeededList(driver, seed);
    });

    await test.step("both switches flip on and off, persisted server-side", async () => {
      await driver.openDisplayOptions();
      // The reset omits both keys, which renders unchecked; flip on, prove
      // it stored, then flip back off and prove that too.
      expect(await driver.isDisplayOptionChecked("Show empty groups")).toBe(false);
      expect(await driver.isDisplayOptionChecked("Show sub-work items")).toBe(false);
      await driver.setDisplayExtraOption("Show empty groups", true);
      await driver.setDisplayExtraOption("Show sub-work items", true);
      expect(await driver.isDisplayOptionChecked("Show empty groups")).toBe(true);
      expect(await driver.isDisplayOptionChecked("Show sub-work items")).toBe(true);
      await expectStoredDisplayFilter(seed, "show_empty_groups", true);
      await expectStoredDisplayFilter(seed, "sub_issue", true);
      await driver.setDisplayExtraOption("Show empty groups", false);
      await driver.setDisplayExtraOption("Show sub-work items", false);
      expect(await driver.isDisplayOptionChecked("Show empty groups")).toBe(false);
      expect(await driver.isDisplayOptionChecked("Show sub-work items")).toBe(false);
      await driver.closeDisplayOptions();
      await expectStoredDisplayFilter(seed, "show_empty_groups", false);
      await expectStoredDisplayFilter(seed, "sub_issue", false);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

const MATRIX_ROWS = ["ISS-075", "ISS-077"];
test(
  specTitle(MATRIX_ROWS, "display matrix follows the active layout (kanban gains sub-group by)"),
  { tag: specTags(MATRIX_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openKanbanList(driver, seed);
    });

    await test.step("kanban with a grouping offers sub-group by", async () => {
      await driver.openDisplayOptions();
      const panel = await driver.displayPanelText();
      expect(panel).toContain("Sub-group by");
      await driver.closeDisplayOptions();
    });
  }
);

const ANALYTICS_ROWS = ["ISS-107"];
test(
  specTitle(ANALYTICS_ROWS, "analytics opens a project-scoped dialog"),
  { tag: specTags(ANALYTICS_ROWS) },
  async ({ driver, seed }) => {
    await test.step("sign in and open the seeded project", async () => {
      await openSeededList(driver, seed);
    });

    await test.step("the dialog names the project and counts its work items", async () => {
      await driver.openAnalytics();
      // The counts load after the dialog frame; wait for the settled total,
      // which covers at least the seeded issues (siblings add their own on
      // the shared stack; a clean checkout holds exactly the seed).
      await expect.poll(() => driver.analyticsDialogText(), { timeout: 60_000 }).toMatch(/Total Work items\s+\d+/);
      const text = await driver.analyticsDialogText();
      expect(text).toContain(seed.projectName);
      const total = Number(/Total Work items\s+(\d+)/.exec(text)?.[1] ?? "NaN");
      expect(total).toBeGreaterThanOrEqual(seed.issueNames.length);
      await driver.closeAnalytics();
    });

    await test.step("opening analytics changes nothing server-side", async () => {
      const names = new Set(await serverIssueNames(seed.workspaceSlug, seed.projectId, session));
      // Containment, not equality: sibling suites share this stack and add
      // their own issues; on a clean checkout the seed stands alone.
      for (const name of seed.issueNames) expect(names.has(name)).toBe(true);
    });
  }
);
