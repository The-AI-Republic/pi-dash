// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: Issues filter dimensions (NEWFRONT-119). Rows ISS-082
// (state), ISS-083 (state group), ISS-084 (priority), ISS-085 (assignee),
// ISS-086 (created by), ISS-087 (mentions), ISS-088 (labels), ISS-089
// (cycle), ISS-090 (module), ISS-091 (project), ISS-092 (start/due dates),
// ISS-093 (subscriber).
//
// SCOPE NOTE (verified by code-wide composer search plus live recon): the
// legacy per-dimension dropdown chrome described in these rows' Acceptance
// (a "Filters" dropdown with collapsible sections, five-row caps, applied
// counts, search boxes, custom-date modal) renders on NO issues surface in
// this checkout — every issues header pairs the condition-row builder with
// the Display popover only. Per the tracker parent (NEWFRONT-28) these
// scenarios prove each dimension's FILTERING BEHAVIOR through the live
// condition-row UI on the project issues page — the parity signal the
// rebuild needs — and are tagged to their rows. The chrome gap itself is
// reported to the parent for an inventory correction. ISS-094 (shared
// section chrome) has no filtering behavior to prove and carries no
// scenario here; see the parent finding.
//
// REVEAL PATTERN (verified live): the row renders only while the stored
// expression is non-empty (NEWFRONT-146), so every scenario seeds an
// expression through the API, then opens the list.
import { test, expect } from "../fixtures";
import {
  resetIssueUserProperties,
  serverProjectViewFlags,
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

/** Seed an expression, open the list, and wait for the row it reveals. */
async function revealRichRow(
  driver: RichDriver,
  seed: SeedFacts,
  expression: Record<string, unknown>,
  pollNames = true
) {
  // Seed AFTER sign-in: the page reads the stored expression at load, and
  // sibling suites share this user — seeding last narrows the overwrite
  // window to seconds. Retry the whole sequence once for the sibling write
  // or entry blip that lands inside it anyway.
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

// Priority=None and state-group unstarted both match every seed issue (all
// three carry priority "none" in the one seeded state, group unstarted).
const REVEAL_KEEP_ALL = { and: [{ priority__in: "none" }] };
const REVEAL_KEEP_ALL_GROUP = { and: [{ state_group__in: "unstarted" }] };

/**
 * Poll the stored expression until it contains the needle. UI writes land
 * after the click returns (debounce plus a hot shared stack), so a single
 * read after a value pick races the in-flight PATCH.
 */
async function expectStoredRichFilters(seed: SeedFacts, needle: string) {
  await expect
    .poll(
      async () =>
        JSON.stringify((await serverUserProperties(seed.workspaceSlug, seed.projectId, session)).rich_filters),
      { timeout: 30_000 }
    )
    .toContain(needle);
}

const PICKER_ROWS = ["ISS-082", "ISS-083", "ISS-085", "ISS-086", "ISS-087", "ISS-088", "ISS-092"];
test(
  specTitle(PICKER_ROWS, "project picker offers the issue dimensions"),
  { tag: specTags(PICKER_ROWS) },
  async ({ driver, seed }) => {
    // Seeded on Priority, so the picker offers every other dimension while
    // the used one stays off (Priority-when-free is proven by the priority
    // test adding it from a state-group seed).
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("member, state, label, and date properties are offered", async () => {
      const options = await driver.listRichPickerOptions();
      for (const name of [
        "State",
        "State Group",
        "Assignees",
        "Created by",
        "Mentions",
        "Label",
        "Start date",
        "Target date",
      ]) {
        expect(options).toContain(name);
      }
    });

    await test.step("the already-used property stays off the picker", async () => {
      const options = await driver.listRichPickerOptions();
      expect(options).not.toContain("Priority");
    });

    await test.step("opening the picker stores nothing", async () => {
      // The picker only lists dimensions; the stored expression still
      // equals the seeded one.
      const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(props.rich_filters).toEqual(REVEAL_KEEP_ALL);
    });
  }
);

const STATE_ROWS = ["ISS-082", "ISS-083"];
test(
  specTitle(STATE_ROWS, "state and state-group dimensions narrow the list"),
  { tag: specTags(STATE_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("filtering state to the seeded state keeps every issue", async () => {
      await driver.addRichCondition("State");
      await driver.pickRichValues(["Todo"]);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      await expectStoredRichFilters(seed, "state");
      // Drop the State condition again so the group step below starts
      // from the seeded row alone (index 0 is the seeded condition).
      await driver.removeRichCondition(1);
      expect(await driver.richConditionCount()).toBe(1);
    });

    await test.step("excluding the seeded group empties the list", async () => {
      // The seed lives in group unstarted, so Backlog matches nothing.
      await driver.addRichCondition("State Group");
      await driver.pickRichValues(["Backlog"]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "backlog");
    });

    await test.step("clearing restores every seeded issue", async () => {
      await driver.clearRichFilters();
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
    });
  }
);

const PRIORITY_ROWS = ["ISS-084"];
test(
  specTitle(PRIORITY_ROWS, "priority dimension narrows to the chosen value"),
  { tag: specTags(PRIORITY_ROWS) },
  async ({ driver, seed }) => {
    // Seeded on the state-group dimension so Priority stays free to add.
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL_GROUP);
    await driver.addRichCondition("Priority");

    await test.step("choosing an absent priority empties the list", async () => {
      await driver.pickRichValues(["Urgent"]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "urgent");
    });
  }
);

const CREATOR_ROWS = ["ISS-086"];
test(
  specTitle(CREATOR_ROWS, "creator dimension uses the member list"),
  { tag: specTags(CREATOR_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("the seeded owner created every issue", async () => {
      await driver.addRichCondition("Created by");
      const options = await driver.richValueOptions();
      const raw = options.find((name) => name.includes("Parity"));
      if (raw === undefined) throw new Error("[parity] seeded owner missing from member options.");
      // Member options prefix the avatar initial in their accessible name
      // ("P Parity Oracle"), so match on the name part without it.
      await driver.pickRichValuesContaining([raw.slice(1)]);
      await expect
        .poll(() => driver.visibleIssueNames(), LIST_POLL)
        .toEqual(expect.arrayContaining([...seed.issueNames]));
      await expectStoredRichFilters(seed, "created_by");
    });
  }
);

const ASSIGNEE_ROWS = ["ISS-085"];
test(
  specTitle(ASSIGNEE_ROWS, "assignee dimension uses the member list"),
  { tag: specTags(ASSIGNEE_ROWS) },
  async ({ driver, seed }) => {
    // Its own reveal: clearing hides the row (ISS-096), so each member
    // dimension starts from a fresh seeded row rather than sharing one.
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("nothing seeded is assigned to the owner", async () => {
      await driver.addRichCondition("Assignees");
      const options = await driver.richValueOptions();
      const raw = options.find((name) => name.includes("Parity"));
      if (raw === undefined) throw new Error("[parity] seeded owner missing from assignee options.");
      // Member options prefix the avatar initial in their accessible name
      // ("P Parity Oracle"), so match on the name part without it.
      await driver.pickRichValuesContaining([raw.slice(1)]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "assignee");
    });
  }
);

const MENTION_ROWS = ["ISS-087"];
test(
  specTitle(MENTION_ROWS, "mention dimension records without matches"),
  { tag: specTags(MENTION_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);

    await test.step("a mention of the owner matches nothing seeded", async () => {
      await driver.addRichCondition("Mentions");
      const options = await driver.richValueOptions();
      const raw = options.find((name) => name.includes("Parity"));
      if (raw === undefined) throw new Error("[parity] seeded owner missing from mention options.");
      // Member options prefix the avatar initial in their accessible name
      // ("P Parity Oracle"), so match on the name part without it.
      await driver.pickRichValuesContaining([raw.slice(1)]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "mention");
    });
  }
);

const LABEL_ROWS = ["ISS-088"];
test(
  specTitle(LABEL_ROWS, "label dimension narrows to labelled work items"),
  { tag: specTags(LABEL_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("Label");

    await test.step("any project label matches nothing seeded", async () => {
      // No seed issue carries a label, so whatever the project offers —
      // including sibling fixtures on the shared stack — matches nothing.
      const options = await driver.richValueOptions();
      if (options.length === 0) {
        // A labelless project leaves the slot empty and the list inert;
        // the valueless condition stores nothing server-side.
        const row = await driver.richFilterRowText();
        expect(row).toContain("Label");
        expect(row).toContain("--");
        await expect
          .poll(() => driver.visibleIssueNames(), LIST_POLL)
          .toEqual(expect.arrayContaining([...seed.issueNames]));
        const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
        expect(JSON.stringify(props.rich_filters)).not.toContain("label");
        return;
      }
      const first = options[0];
      if (first === undefined) throw new Error("[parity] label options vanished mid-read.");
      await driver.pickRichValues([first]);
      await expect.poll(() => driver.visibleIssueNames(), LIST_POLL).not.toContain(seed.issueNames[0]);
      await expectStoredRichFilters(seed, "label");
    });
  }
);

const DATE_DIM_ROWS = ["ISS-092"];
test(
  specTitle(DATE_DIM_ROWS, "start-date dimension narrows the list"),
  { tag: specTags(DATE_DIM_ROWS) },
  async ({ driver, seed }) => {
    await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
    await driver.addRichCondition("Start date");

    await test.step("picking a day empties the dateless seed", async () => {
      // No seed issue carries a start date, so any picked day must empty
      // the visible list.
      await driver.pickRichDay("15");
      await expectStoredRichFilters(seed, "start_date");
      const names = await driver.visibleIssueNames();
      for (const seeded of seed.issueNames) expect(names).not.toContain(seeded);
    });
  }
);

const GATING_ROWS = ["ISS-089", "ISS-090", "ISS-091", "ISS-093"];
test(
  specTitle(GATING_ROWS, "cycle, module, project, and subscriber stay off the project picker"),
  { tag: specTags(GATING_ROWS) },
  async ({ driver, seed }) => {
    // Cycle/Module need their project features; Projects/Subscriber belong
    // to cross-project and personal views. Pin the features off (restoring
    // whatever the shared stack had) and prove none is offered here.
    const flags = await serverProjectViewFlags(seed.workspaceSlug, seed.projectId, session);
    await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, {
      issue_views_view: false,
      cycle_view: false,
      module_view: false,
    });
    try {
      await revealRichRow(driver, seed, REVEAL_KEEP_ALL);
      const options = await driver.listRichPickerOptions();
      for (const name of ["Cycle", "Module", "Projects", "Subscriber"]) {
        expect(options).not.toContain(name);
      }
      // The pinned features held through the page load and the stored
      // expression is still the seeded one.
      const pinned = await serverProjectViewFlags(seed.workspaceSlug, seed.projectId, session);
      expect(pinned.issue_views_view).toBe(false);
      expect(pinned.cycle_view).toBe(false);
      expect(pinned.module_view).toBe(false);
      const props = await serverUserProperties(seed.workspaceSlug, seed.projectId, session);
      expect(props.rich_filters).toEqual(REVEAL_KEEP_ALL);
    } finally {
      await setProjectViewFlags(seed.workspaceSlug, seed.projectId, session, {
        issue_views_view: flags.issue_views_view,
        cycle_view: flags.cycle_view,
        module_view: flags.module_view,
      });
    }
  }
);
