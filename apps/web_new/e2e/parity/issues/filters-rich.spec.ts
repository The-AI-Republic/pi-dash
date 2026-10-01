// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: Issues condition-row builder (NEWFRONT-119). Rows
// ISS-096 (toggle), ISS-097 (condition row), ISS-098 (field picker and AND),
// ISS-099 (negation), ISS-100 (single pick), ISS-101 (multi pick),
// ISS-102 (single date), ISS-103 (date range), ISS-104 (clear all, save and
// update view), ISS-105 (persisted project instances). Written from the
// area spec parity/specs/issues-filters-display.md against the driver
// interface; the oracle driver implements the reads from the running old
// app (extend, never fork).
//
// REVEAL PATTERN (verified live): the row renders only while the stored
// expression is non-empty — the header toggle cannot conjure it from an
// empty store (NEWFRONT-146). Every scenario therefore seeds an expression
// through the API, then opens the list; the row is already there and the
// scenario drives it from that point. The seeded Priority=None expression
// keeps every seed issue (all three carry priority "none"); the seeded
// Urgent expression matches nothing and proves AND-join behavior.
import { test, expect } from "../fixtures";
import {
  createProjectView,
  deleteProjectView,
  resetIssueUserProperties,
  serverIssueNames,
  serverProjectViewFlags,
  serverSavedViews,
  serverUserProperties,
  setIssueRichFilters,
  setProjectViewFlags,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

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

// Leave the shared oracle user clean for sibling suites: every test seeds
// rich_filters, and a leftover (e.g. priority=urgent) blanks the project
// list for anyone else on the stack (NEWFRONT-121). Runs even on failure.
test.afterEach(async ({ seed }) => {
  await resetIssueUserProperties(seed.workspaceSlug, seed.projectId, session);
});

type RichDriver = {
  openEntry(): Promise<void>;
  signInWithPassword(email: string, password: string): Promise<void>;
  openProjectIssues(workspaceSlug: string, projectId: string): Promise<void>;
  openProjectView(workspaceSlug: string, projectId: string, viewId: string): Promise<void>;
  visibleIssueNames(): Promise<string[]>;
  isRichFilterRowVisible(): Promise<boolean>;
};

type SeedFacts = {
  email: string;
  password: string;
  workspaceSlug: string;
  projectId: string;
  issueNames: string[];
};

async function openSeededList(driver: RichDriver, seed: SeedFacts) {
  // Reset AFTER sign-in: the page reads the stored state at load, and
  // sibling suites share this user — resetting last narrows the overwrite
  // window to seconds. Retry the whole sequence once for the sibling write
  // or entry blip that lands inside it anyway.
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

/** Seed an expression, open the list, and wait for the row it reveals. */
async function revealRichRow(
  driver: RichDriver,
  seed: SeedFacts,
  expression: Record<string, unknown>,
  pollNames = true
) {
  // Seed AFTER sign-in for the same shared-user reason as openSeededList,
  // with the same single retry of the whole sequence.
  const attempt = async () => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await setIssueRichFilters(seed.workspaceSlug, seed.projectId, session, expression);
    await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
    if (pollNames) {
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    }
    await expect.poll(() => driver.isRichFilterRowVisible(), { timeout: 60_000 }).toBe(true);
  };
  try {
    await attempt();
  } catch {
    await attempt();
  }
}

// Priority=None matches every seed issue; Urgent matches none. Both shapes
// are the verified {and:[{prop__in:value}]} store form.
const REVEAL_KEEP_ALL = { and: [{ priority__in: "none" }] };
const REVEAL_MATCH_NOTHING = { and: [{ priority__in: "urgent" }] };

/**
 * Poll the stored expression until it contains every needle. UI writes land
 * after the click returns (debounce plus a hot shared stack), so a single
 * read after a value pick races the in-flight PATCH.
 */
async function expectStoredRichFilters(seed: SeedFacts, ...needles: string[]) {
  for (const needle of needles) {
    await expect
      .poll(
        async () =>
          JSON.stringify((await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).rich_filters),
        { timeout: 30_000 }
      )
      .toContain(needle);
  }
}

/** Poll until the stored expression is empty (clear-all writes land late too). */
async function expectStoredRichFiltersEmpty(seed: SeedFacts) {
  await expect
    .poll(
      async () =>
        Object.keys((await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).rich_filters).length,
      { timeout: 30_000 }
    )
    .toBe(0);
}

const TOGGLE_ROWS = ["ISS-096"];
test(
  specTitle(TOGGLE_ROWS, "bug: header toggle is inert; the row follows the expression (NEWFRONT-146)"),
  { tag: specTags(TOGGLE_ROWS) },
  async ({ driver, seed }) => {
    // The row wants the header toggle to show/hide the row; the oracle's
    // toggle does nothing in either direction (146), so the row follows
    // the stored expression instead: a seeded expression shows it and
    // clearing the last condition hides it.
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("the seeded expression shows the row; the toggle leaves it", async () => {
      expect(await driver.richConditionCount()).toBe(1);
      const row = await driver.richFilterRowText();
      expect(row).toContain("Priority");
      await driver.toggleRichFilterRow();
      await driver.toggleRichFilterRow();
      expect(await driver.isRichFilterRowVisible()).toBe(true);
      const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(JSON.stringify(props.rich_filters)).toContain("priority");
    });

    await test.step("clearing the last condition hides the row", async () => {
      await driver.clearRichFilters();
      // The row hides on the re-render after the clear PATCH lands.
      await expect.poll(() => driver.isRichFilterRowVisible(), { timeout: 60_000 }).toBe(false);
      await expectStoredRichFiltersEmpty(seed);
    });
  }
);

const ADD_ROWS = ["ISS-097", "ISS-098"];
test(
  specTitle(ADD_ROWS, "field picker adds conditions joined with AND"),
  { tag: specTags(ADD_ROWS) },
  async ({ driver, seed }) => {
    // The seeded Urgent condition matches nothing, so the row opens over an
    // empty list; adding State=Todo (matches all) must leave it empty —
    // that is the AND join, where OR would restore every issue.
    await revealRichRow(driver, seed, REVEAL_MATCH_NOTHING, false);

    await test.step("a second condition joins the seeded one", async () => {
      await driver.addRichCondition("State");
      expect(await driver.richConditionCount()).toBe(2);
      const row = await driver.richFilterRowText();
      expect(row).toContain("Priority");
      expect(row).toContain("State");
    });

    await test.step("both conditions apply as an intersection", async () => {
      await driver.pickRichValues(["Todo"]);
      const names = await driver.visibleIssueNames();
      for (const seeded of seed.issueNames) expect(names).not.toContain(seeded);
      await expectStoredRichFilters(seed, "priority", "state");
    });

    await test.step("removing drops just that condition", async () => {
      await driver.removeRichCondition(0);
      expect(await driver.richConditionCount()).toBe(1);
      const row = await driver.richFilterRowText();
      expect(row).toContain("State");
    });
  }
);

const VALUE_ROWS = ["ISS-101"];
test(
  specTitle(VALUE_ROWS, "multi pick values drive the list"),
  { tag: specTags(VALUE_ROWS) },
  async ({ driver, seed }) => {
    // Seeded on the state-group dimension so Priority stays free to add.
    await revealRichRow(driver, seed, { and: [{ state_group__in: "unstarted" }] });
    await driver.addRichCondition("Priority");

    await test.step("the value slot offers the fixed priority set", async () => {
      const options = await driver.richValueOptions();
      for (const name of ["Urgent", "High", "Medium", "Low"]) expect(options).toContain(name);
    });

    await test.step("a priority value the seed lacks empties the list", async () => {
      // The seed carries no urgent work item, so choosing Urgent must empty
      // the visible list while the server keeps all three rows.
      await driver.pickRichValues(["Urgent"]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "urgent");
      const names = new Set(await serverIssueNames(seed.workspaceSlug, seed.projectId, session));
      // Containment, not equality: sibling suites share this stack and add
      // their own issues; on a clean checkout the seed stands alone.
      for (const name of seed.issueNames) expect(names.has(name)).toBe(true);
    });
  }
);

const NEGATION_ROWS = ["ISS-099"];
test(
  specTitle(NEGATION_ROWS, "bug: priority operator is locked to Is with no negated form (NEWFRONT-149)"),
  { tag: specTags(NEGATION_ROWS) },
  async ({ driver, seed }) => {
    // The row wants positive and negated operator forms (is / is not); the
    // oracle locks single-operator properties instead, so no menu opens.
    await revealRichRow(driver, seed, { and: [{ state_group__in: "unstarted" }] });
    await driver.addRichCondition("Priority");
    // Value before removing the seeded condition: valueless conditions are
    // UI-ephemeral and collapse away on the next expression write.
    await driver.pickRichValues(["Urgent"]);
    await driver.removeRichCondition(0);

    await test.step("the lone priority condition offers no operator menu", async () => {
      expect(await driver.richConditionCount()).toBe(1);
      expect(await driver.isSingleRichOperatorLocked()).toBe(true);
      const row = await driver.richFilterRowText();
      expect(row).toContain("Priority");
      expect(row).toContain("Urgent");
      // The locked operator still stores the picked value normally.
      await expectStoredRichFilters(seed, "priority", "urgent");
    });
  }
);

const SINGLE_ROWS = ["ISS-100"];
test(
  specTitle(SINGLE_ROWS, "single pick keeps matching work items"),
  { tag: specTags(SINGLE_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("filtering state to the seeded state keeps every issue", async () => {
      await driver.addRichCondition("State");
      const options = await driver.richValueOptions();
      expect(options).toContain("Todo");
      await driver.pickRichValues(["Todo"]);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      await expectStoredRichFilters(seed, "state");
    });
  }
);

const DATE_ROWS = ["ISS-102"];
test(
  specTitle(DATE_ROWS, "single date opens a calendar and stores the picked day"),
  { tag: specTags(DATE_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("Target date");

    await test.step("the empty date slot opens its calendar by itself", async () => {
      expect(await driver.richConditionCount()).toBe(2);
      const row = await driver.richFilterRowText();
      expect(row).toContain("Target date");
      expect(row).toContain("--");
      await expect.poll(() => driver.isRichCalendarOpen(), { timeout: 15_000 }).toBe(true);
    });

    await test.step("picking a day stores it and empties the dateless seed", async () => {
      // No seed issue carries a target date, so any picked day must empty
      // the visible list. Value before removing the seeded condition:
      // valueless conditions collapse away on the next expression write.
      await driver.pickRichDay("15");
      await driver.removeRichCondition(0);
      expect(await driver.richConditionCount()).toBe(1);
      await expectStoredRichFilters(seed, "target_date");
      const names = await driver.visibleIssueNames();
      for (const seeded of seed.issueNames) expect(names).not.toContain(seeded);
    });
  }
);

const RANGE_ROWS = ["ISS-103"];
test(
  specTitle(RANGE_ROWS, "date range commits both ends and narrows the list"),
  { tag: specTags(RANGE_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("Start date");
    // Value under the default operator first: valueless conditions
    // collapse away on the next expression write.
    await driver.pickRichDay("15");
    await driver.removeRichCondition(0);

    await test.step("the date operator offers a between form", async () => {
      expect(await driver.richConditionCount()).toBe(1);
      expect(await driver.isSingleRichOperatorLocked()).toBe(false);
      const options = await driver.richOperatorOptions();
      expect(options).toContain("between");
      await driver.pickRichOperator("between");
      const row = await driver.richFilterRowText();
      expect(row).toContain("between");
    });

    await test.step("picking both ends stores the range and empties the seed", async () => {
      // No seed issue carries a start date, so any range must empty the
      // visible list once both ends commit. The incomplete range reopens
      // its own calendar for the missing end.
      await expect.poll(() => driver.isRichCalendarOpen(), { timeout: 15_000 }).toBe(true);
      await driver.pickRichDay("17");
      await expectStoredRichFilters(seed, "start_date");
      const names = await driver.visibleIssueNames();
      for (const seeded of seed.issueNames) expect(names).not.toContain(seeded);
    });
  }
);

const CLEAR_ROWS = ["ISS-104"];
test(
  specTitle(CLEAR_ROWS, "clear all drops every condition and restores the list"),
  { tag: specTags(CLEAR_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_MATCH_NOTHING, false);
    await driver.addRichCondition("State");
    await driver.pickRichValues(["Todo"]);
    expect(await driver.richConditionCount()).toBe(2);

    await test.step("clear all restores the full list", async () => {
      await driver.clearRichFilters();
      // The row hides on the re-render after the clear PATCH lands.
      await expect.poll(() => driver.isRichFilterRowVisible(), { timeout: 60_000 }).toBe(false);
      expect(await driver.richConditionCount()).toBe(0);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      await expectStoredRichFiltersEmpty(seed);
    });
  }
);

const SAVE_ROWS = ["ISS-104"];
test(
  specTitle(SAVE_ROWS, "save view stores the expression as a new view"),
  { tag: specTags(SAVE_ROWS) },
  async ({ driver, seed }) => {
    // Project save needs the views feature, off on a clean seed; flip it
    // for the test window and restore whatever was there.
    const flags = await serverProjectViewFlags(seed.workspaceSlug, seed.projectId, session);
    await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, { issue_views_view: true });
    // The created view id, deleted in the finally so an assertion failure
    // cannot leak a view onto the shared stack.
    let savedId: string | undefined;
    try {
      await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
      await driver.addRichCondition("State");
      await driver.pickRichValues(["Todo"]);

      await test.step("saving opens a prefilled dialog and creates the view", async () => {
        await driver.saveRichViewAs("Oracle saved view");
        const views = await serverSavedViews(seed.workspaceSlug, seed.projectId, session);
        const saved = views.find((view) => view.name === "Oracle saved view");
        expect(saved).toBeDefined();
        savedId = saved?.id;
        expect(JSON.stringify(saved?.rich_filters)).toContain("state");
      });
    } finally {
      if (savedId !== undefined) {
        await deleteProjectView(seed.workspaceSlug, seed.projectId, savedId, session);
      }
      await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, {
        issue_views_view: flags.issue_views_view,
      });
    }
  }
);

test(
  specTitle(SAVE_ROWS, "update view writes changed conditions back"),
  { tag: specTags(SAVE_ROWS) },
  async ({ driver, seed }) => {
    const flags = await serverProjectViewFlags(seed.workspaceSlug, seed.projectId, session);
    await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, { issue_views_view: true });
    const viewId = await createProjectView(seed.workspaceSlug, seed.projectId, session, "Oracle update view", {
      and: [{ priority__in: "none" }],
    });
    try {
      await openSeededList(driver, seed);
      // Open the view with one retry: sibling suites flip project flags on
      // the shared stack, and the row stays hidden when the page loads in a
      // flag-off window — so re-assert the flag and reopen before failing.
      const openView = async () => {
        await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, { issue_views_view: true });
        await driver.openProjectView(seed.workspaceSlug, seed.projectId, viewId);
        await expect.poll(() => driver.isRichFilterRowVisible(), { timeout: 60_000 }).toBe(true);
      };
      try {
        await openView();
      } catch {
        await openView();
      }

      await test.step("editing the saved view offers an update that persists", async () => {
        await driver.addRichCondition("State");
        await driver.pickRichValues(["Todo"]);
        await driver.updateRichView();
        // Poll: the update PATCH lands after the click returns.
        await expect
          .poll(
            async () =>
              JSON.stringify(
                (await serverSavedViews(seed.workspaceSlug, seed.projectId, session)).find(
                  (view) => view.name === "Oracle update view"
                )?.rich_filters ?? null
              ),
            { timeout: 30_000 }
          )
          .toContain("state");
      });
    } finally {
      await deleteProjectView(seed.workspaceSlug, seed.projectId, viewId, session);
      await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, {
        issue_views_view: flags.issue_views_view,
      });
    }
  }
);

const REMOVE_ROWS = ["ISS-095"];
test(
  specTitle(REMOVE_ROWS, "bug: removal runs per-condition with no chip bar (NEWFRONT-148)"),
  { tag: specTags(REMOVE_ROWS) },
  async ({ driver, seed }) => {
    // The row wants one chip per active value with its own X; the oracle
    // renders no chip bar, so removal happens through each condition's own
    // remove control while the sibling condition survives.
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("State");
    await driver.pickRichValues(["Todo"]);
    expect(await driver.richConditionCount()).toBe(2);

    await test.step("removing one condition keeps the other and its stored key", async () => {
      await driver.removeRichCondition(0);
      expect(await driver.richConditionCount()).toBe(1);
      const row = await driver.richFilterRowText();
      expect(row).toContain("State");
      await expect
        .poll(
          async () =>
            JSON.stringify((await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).rich_filters),
          { timeout: 30_000 }
        )
        .not.toContain("priority");
      const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(JSON.stringify(props.rich_filters)).toContain("state");
    });
  }
);

const PERSIST_ROWS = ["ISS-105", "ISS-106"];
test(
  specTitle(PERSIST_ROWS, "project conditions persist across reload outside the URL"),
  { tag: specTags(PERSIST_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("State");
    await driver.pickRichValues(["Todo"]);
    expect(await driver.richConditionCount()).toBe(2);

    await test.step("the stored expression survives a full reload", async () => {
      await expectStoredRichFilters(seed, "state");
      const before = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.isRichFilterRowVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.richConditionCount()).toBe(2);
      const after = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(after.rich_filters).toEqual(before.rich_filters);
    });

    await test.step("the filter set stays personal, not deep-linkable", async () => {
      expect(new URL(driver.page.url()).search).not.toContain("filter");
    });
  }
);
