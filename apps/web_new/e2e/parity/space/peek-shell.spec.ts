// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: space issue peek shell (NEWFRONT-450). Rows SPC-040–046:
// address-opened overlays, the three presentations with their switcher,
// close semantics, the key/title/description block, the property panel,
// clipboard-gated copy-link with its notice, and loading skeletons.
// Green on the old space app served per NEWFRONT-97.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  projectFacts,
  serverSpaceCleanupProbeLabels,
  serverSpaceCleanupProbes,
  serverSpaceCreateIssue,
  serverSpaceIssueDetail,
  serverSpaceIssues,
  serverSpaceSetBoardFlags,
  serverSpaceStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.setTimeout(480_000);
const BOARD_POLL = { timeout: 120_000 };

const ISSUE_PREFIX = "SPC-450 probe ";
const LABEL_PREFIX = "SPC-450 probe label ";

let session = "";

function requireAnchor(seed: ParitySeedFacts): string {
  if (!seed.spaceAnchor) {
    throw new Error("[parity] seed facts carry no spaceAnchor; re-run the stack seed step.");
  }
  return seed.spaceAnchor;
}

async function resetProbes(seed: ParitySeedFacts): Promise<void> {
  if (session === "") return;
  await serverSpaceCleanupProbes(seed.workspaceSlug, seed.projectId, session, ISSUE_PREFIX);
  await serverSpaceCleanupProbeLabels(seed.workspaceSlug, seed.projectId, session, LABEL_PREFIX);
  // The flag-flip scenario restores in a finally; this keeps reruns
  // convergent when a run dies mid-flip.
  await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, {
    comments: false,
    reactions: false,
    votes: false,
  });
}

// Convergent reruns on one seeded stack: drop leftovers from an
// interrupted run before each scenario, and again after.
test.beforeEach(async ({ seed }) => {
  session = await signInSession(seed.email, seed.password);
  await resetProbes(seed);
});

test.afterEach(async ({ seed }) => {
  await resetProbes(seed);
});

/** Board issue titles settle to exactly the expected set, order-insensitive. */
async function expectBoardNames(driver: ParityDriver, expected: string[]): Promise<void> {
  const want = [...expected].sort();
  await expect.poll(async () => (await driver.spaceFiltersVisibleIssueNames()).sort(), BOARD_POLL).toEqual(want);
}

/** The overlay mounts after boot; poll for it after every address open. */
async function expectPeekOpen(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.spacePeekOpen(), BOARD_POLL).toBe(true);
}

/**
 * The overlay mounts instantly with skeletons while the issue data loads
 * (the list fetch can seed it ahead of the detail fetch); poll for
 * content before reading key/title/description/properties.
 */
async function expectPeekDetail(driver: ParityDriver): Promise<void> {
  await expect.poll(() => driver.spacePeekTitle(), BOARD_POLL).not.toBeNull();
}

async function seedIssueId(anchor: string, seed: ParitySeedFacts, index: number): Promise<string> {
  const issue = (await serverSpaceIssues(anchor)).find((row) => row.name === seed.issueNames[index]);
  if (!issue) throw new Error(`[parity] seeded issue ${seed.issueNames[index]} missing from the public list.`);
  return issue.id;
}

test(
  specTitle(["SPC-040"], "issue address loads the board with that issue's overlay open"),
  { tag: specTags(["SPC-040"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueId = await seedIssueId(anchor, seed, 0);

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    expect(await driver.spacePeekMode()).toBe("side");

    // The overlay shows the served detail, not a stale copy.
    const detail = await serverSpaceIssueDetail(anchor, issueId);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    expect(await driver.spacePeekTitle()).toBe(detail.name);
    expect(await driver.spacePeekKey()).toBe(`${identifier}-${detail.sequenceId}`);

    // The board loads behind the overlay.
    await expectBoardNames(driver, seed.issueNames);
  }
);

test(
  specTitle(["SPC-041"], "overlay offers side, modal and full presentations with a preserving switcher"),
  { tag: specTags(["SPC-041"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueId = await seedIssueId(anchor, seed, 1);

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    expect(await driver.spacePeekMode()).toBe("side");
    const key = await driver.spacePeekKey();
    const title = await driver.spacePeekTitle();
    expect(await driver.spacePeekProperties()).not.toBeNull();

    await driver.spacePeekSwitchMode("modal");
    expect(await driver.spacePeekMode()).toBe("modal");
    expect(await driver.spacePeekKey()).toBe(key);
    expect(await driver.spacePeekTitle()).toBe(title);
    expect(await driver.spacePeekProperties()).not.toBeNull();

    await driver.spacePeekSwitchMode("full");
    expect(await driver.spacePeekMode()).toBe("full");
    expect(await driver.spacePeekKey()).toBe(key);
    expect(await driver.spacePeekTitle()).toBe(title);
    const fullProps = await driver.spacePeekProperties();
    expect(fullProps).not.toBeNull();
    expect(fullProps?.state).toBe("Todo");

    // The address still names the same issue throughout.
    expect(new URL(driver.page.url()).searchParams.get("peekId")).toBe(issueId);

    await driver.spacePeekSwitchMode("side");
    expect(await driver.spacePeekMode()).toBe("side");
    expect(await driver.spacePeekKey()).toBe(key);
    expect(await driver.spacePeekTitle()).toBe(title);
  }
);

test(
  specTitle(["SPC-041"], "bug: full-screen overlay mounts the activity section while comments are disabled"),
  { tag: specTags(["SPC-041"]) },
  async ({ driver, seed }) => {
    // Intended: commenting disabled hides the activity section in every
    // presentation (SPC-049). The side view gates on the flag; full-screen
    // mounts unconditionally (open confirmation 3). See NEWFRONT-461,
    // filed by the engagement area which owns the target behavior.
    const anchor = requireAnchor(seed);
    const issueId = await seedIssueId(anchor, seed, 2);

    if (driver.target === "web_new") {
      // Intended behavior (NEWFRONT-461): commenting disabled hides the
      // activity section in every presentation; enabling shows it in side
      // view too, proving one flag gates them all.
      await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
      await expectPeekOpen(driver);
      expect(await driver.spacePeekMode()).toBe("side");
      expect(await driver.spacePeekActivityVisible()).toBe(false);

      await driver.spacePeekSwitchMode("full");
      await expect.poll(() => driver.spacePeekMode(), BOARD_POLL).toBe("full");
      expect(await driver.spacePeekActivityVisible()).toBe(false);

      await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, { comments: true });
      try {
        await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
        await expectPeekOpen(driver);
        expect(await driver.spacePeekMode()).toBe("side");
        await expect.poll(() => driver.spacePeekActivityVisible(), BOARD_POLL).toBe(true);
      } finally {
        await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, { comments: false });
      }
      return;
    }

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
    await expectPeekOpen(driver);
    expect(await driver.spacePeekMode()).toBe("side");
    expect(await driver.spacePeekActivityVisible()).toBe(false);

    await driver.spacePeekSwitchMode("full");
    expect(await driver.spacePeekActivityVisible()).toBe(true);

    // Enabling comments shows the section in side view too, proving the
    // side gate reads the same flag full-screen ignores.
    await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, { comments: true });
    try {
      await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
      await expectPeekOpen(driver);
      expect(await driver.spacePeekMode()).toBe("side");
      await expect.poll(() => driver.spacePeekActivityVisible(), BOARD_POLL).toBe(true);
    } finally {
      await serverSpaceSetBoardFlags(seed.workspaceSlug, seed.projectId, session, { comments: false });
    }
  }
);

test(
  specTitle(["SPC-042"], "closing the overlay drops the issue from the address and keeps view plus filters"),
  { tag: specTags(["SPC-042"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const probe = `${ISSUE_PREFIX}close`;
    const probeId = await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, {
      name: probe,
      priority: "high",
    });
    const query = `?board=list&priority=high&peekId=${probeId}`;

    // Each presentation closes through its own mechanism: the header
    // control in side view, the backdrop in modal, Escape in full-screen.
    // The pasted priority address seeds the pill on both apps; whether the
    // board underneath is already narrowed depends on the app (old bug
    // NEWFRONT-463: the old load fetch drops the seeded filter, while
    // web_new carries it). Close must preserve whichever board it opened
    // over — it only drops the issue.
    const boardBehind = driver.target === "web_new" ? [probe] : [...seed.issueNames, probe];
    for (const [mode, via] of [
      ["side", "header"],
      ["modal", "backdrop"],
      ["full", "escape"],
    ] as const) {
      await driver.spaceOpenBoard(anchor, query);
      await expectPeekOpen(driver);
      if (mode !== "side") await driver.spacePeekSwitchMode(mode);
      await driver.spacePeekClose(via);

      expect(await driver.spacePeekOpen()).toBe(false);
      const url = new URL(driver.page.url());
      expect(url.searchParams.has("peekId")).toBe(false);
      expect(url.searchParams.get("board")).toBe("list");
      expect(url.searchParams.get("priority")).toBe("high");
      expect(await driver.spaceFiltersAppliedFacets()).toEqual([{ facet: "priority", values: ["high"] }]);
      await expectBoardNames(driver, boardBehind);
    }

    // Server-side the surviving parameters still narrow to the probe.
    expect((await serverSpaceIssues(anchor, { priority: "high" })).map((issue) => issue.name)).toEqual([probe]);
  }
);

test(
  specTitle(["SPC-043"], "overlay shows key, title and read-only formatted description; empty hides it"),
  { tag: specTags(["SPC-043"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const probe = `${ISSUE_PREFIX}described`;
    const probeId = await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, {
      name: probe,
      descriptionHtml: "<p>Probe <strong>bold</strong> tail</p>",
    });
    const detail = await serverSpaceIssueDetail(anchor, probeId);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${probeId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    expect(await driver.spacePeekKey()).toBe(`${identifier}-${detail.sequenceId}`);
    expect(await driver.spacePeekTitle()).toBe(probe);
    // The description arrives with the detail fetch (the list seed carries
    // none), so poll for the block itself.
    await expect.poll(() => driver.spacePeekDescriptionVisible(), BOARD_POLL).toBe(true);
    expect(await driver.spacePeekDescriptionText()).toContain("Probe bold tail");
    expect(await driver.spacePeekDescriptionHtml()).toContain("<strong>");
    expect(await driver.spacePeekDescriptionEditable()).toBe(false);
    expect(detail.descriptionHtml ?? "").toContain("bold");

    // An issue without a description shows no description block at all.
    const bareId = await seedIssueId(anchor, seed, 0);
    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${bareId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    expect(await driver.spacePeekTitle()).toBe(seed.issueNames[0]);
    expect(await driver.spacePeekKey()).not.toBeNull();
    expect(await driver.spacePeekDescriptionVisible()).toBe(false);
    expect(await driver.spacePeekDescriptionText()).toBeNull();
  }
);

test(
  specTitle(["SPC-044"], "property panel shows state, priority and due date with overdue emphasis"),
  { tag: specTags(["SPC-044"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const overdue = `${ISSUE_PREFIX}overdue`;
    const upcoming = `${ISSUE_PREFIX}upcoming`;
    const overdueId = await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, {
      name: overdue,
      priority: "high",
      targetDate: "2020-01-15",
    });
    const upcomingId = await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, {
      name: upcoming,
      priority: "low",
      targetDate: "2030-06-30",
    });

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${overdueId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    // The state name arrives with the states fetch, behind the detail, so
    // poll the whole panel instead of reading it once.
    await expect
      .poll(() => driver.spacePeekProperties(), BOARD_POLL)
      .toEqual({
        state: "Todo",
        priority: "High",
        dueDate: "Jan 15, 2020",
        dueDateEmphasised: true,
      });

    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${upcomingId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    await expect
      .poll(() => driver.spacePeekProperties(), BOARD_POLL)
      .toEqual({
        state: "Todo",
        priority: "Low",
        dueDate: "Jun 30, 2030",
        dueDateEmphasised: false,
      });

    // Issues without a due date show the empty branch, never emphasised.
    const bareId = await seedIssueId(anchor, seed, 0);
    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${bareId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    await expect
      .poll(() => driver.spacePeekProperties(), BOARD_POLL)
      .toEqual({
        state: "Todo",
        priority: "None",
        dueDate: null,
        dueDateEmphasised: false,
      });

    // Server-side the dates and state match what the panel showed.
    const overDetail = await serverSpaceIssueDetail(anchor, overdueId);
    expect(overDetail.targetDate).toBe("2020-01-15");
    const states = await serverSpaceStates(anchor);
    expect(states.find((state) => state.id === overDetail.stateId)?.name).toBe("Todo");
  }
);

test(
  specTitle(["SPC-045"], "copy-link is gated by clipboard permission and confirmed by notice"),
  { tag: specTags(["SPC-045"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueId = await seedIssueId(anchor, seed, 0);

    // Denied (the headless default): the header offers no copy action.
    // Read after the detail settles so the permission check has resolved.
    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
    await expectPeekOpen(driver);
    await expectPeekDetail(driver);
    expect(await driver.spacePeekCopyLinkVisible()).toBe(false);

    // Granted before load: the header offers it; copying the current
    // address lands on the clipboard and raises a confirmation notice.
    await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);
    await expectPeekOpen(driver);
    await expect.poll(() => driver.spacePeekCopyLinkVisible(), BOARD_POLL).toBe(true);
    const want = driver.page.url();
    await driver.spacePeekCopyLink();
    await expect.poll(() => driver.spacePeekReadClipboard(), BOARD_POLL).toBe(want);
    await expect.poll(() => driver.spacePeekLastNotice(), BOARD_POLL).not.toBeNull();
    const notice = await driver.spacePeekLastNotice();
    expect(`${notice?.title} ${notice?.message}`).toMatch(/copi/i);

    // Modal keeps the header action; full-screen drops it.
    await driver.spacePeekSwitchMode("modal");
    expect(await driver.spacePeekCopyLinkVisible()).toBe(true);
    await driver.spacePeekSwitchMode("full");
    expect(await driver.spacePeekCopyLinkVisible()).toBe(false);
  }
);

test(
  specTitle(["SPC-046"], "overlay shows skeletons while the issue detail resolves"),
  { tag: specTags(["SPC-046"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issueId = await seedIssueId(anchor, seed, 0);

    // Hold every issue-data fetch until the skeletons are proven: the list
    // fetch seeds the overlay store ahead of the detail fetch, so holding
    // the detail alone still lets content replace the skeletons early.
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const issueData = (url: URL): boolean => url.pathname.startsWith(`/api/public/anchor/${anchor}/issues/`);
    await driver.page.route(issueData, async (route) => {
      await gate;
      await route.continue();
    });
    try {
      await driver.spaceOpenBoard(anchor, `?board=list&peekId=${issueId}`);

      // Skeletons first, no half-rendered content…
      await expect.poll(() => driver.spacePeekSkeletonsVisible(), BOARD_POLL).toBe(true);
      expect(await driver.spacePeekTitle()).toBeNull();

      // …in the full presentation too, over the same pending detail…
      await driver.spacePeekSwitchMode("full");
      expect(await driver.spacePeekSkeletonsVisible()).toBe(true);
      expect(await driver.spacePeekTitle()).toBeNull();

      // …then the real content replaces them.
      release();
      await expect.poll(() => driver.spacePeekTitle(), BOARD_POLL).toBe(seed.issueNames[0]);
      expect(await driver.spacePeekSkeletonsVisible()).toBe(false);
    } finally {
      await driver.page.unroute(issueData);
    }
  }
);
