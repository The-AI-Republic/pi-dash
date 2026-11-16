// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: space board filters (NEWFRONT-450). Rows SPC-034–039:
// the filter panel's facets and in-panel search, applied pills with three
// removal levels synced to the address, address-encoded filter state, the
// address-only label filter, per-layout facet sets, and the absence of
// free-text issue search. Green on the old space app served per NEWFRONT-97.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  createProjectLabel,
  createState,
  deleteState,
  projectStates,
  serverSpaceCleanupProbeLabels,
  serverSpaceCleanupProbes,
  serverSpaceCreateIssue,
  serverSpaceIssues,
  serverSpaceSettings,
  serverSpaceStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.setTimeout(480_000);
const BOARD_POLL = { timeout: 120_000 };

const ISSUE_PREFIX = "SPC-450 probe ";
const LABEL_PREFIX = "SPC-450 probe label ";
const STATE_PREFIX = "SPC-450 probe state ";

let session = "";

function requireAnchor(seed: ParitySeedFacts): string {
  if (!seed.spaceAnchor) {
    throw new Error("[parity] seed facts carry no spaceAnchor; re-run the stack seed step.");
  }
  return seed.spaceAnchor;
}

async function cleanupProbeStates(seed: ParitySeedFacts): Promise<void> {
  if (session === "") return;
  for (const state of await projectStates(seed.workspaceSlug, seed.projectId, session)) {
    if (state.name.startsWith(STATE_PREFIX)) {
      await deleteState(seed.workspaceSlug, seed.projectId, state.id, session);
    }
  }
}

async function resetProbes(seed: ParitySeedFacts): Promise<void> {
  if (session === "") return;
  await serverSpaceCleanupProbes(seed.workspaceSlug, seed.projectId, session, ISSUE_PREFIX);
  await serverSpaceCleanupProbeLabels(seed.workspaceSlug, seed.projectId, session, LABEL_PREFIX);
  await cleanupProbeStates(seed);
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

test(
  specTitle(["SPC-034"], "filter panel offers priority and state facets with in-panel search; no label facet"),
  { tag: specTags(["SPC-034"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await driver.spaceOpenBoard(anchor, "?board=list");
    await expectBoardNames(driver, seed.issueNames);

    await driver.spaceFiltersOpenPanel();
    expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);

    const priorities = await driver.spaceFiltersPriorityOptions();
    expect(priorities.map((option) => option.name)).toEqual(["Urgent", "High", "Medium", "Low", "None"]);
    expect(priorities.every((option) => !option.checked)).toBe(true);

    const states = await driver.spaceFiltersStateOptions();
    const serverStates = await serverSpaceStates(anchor);
    expect(states.map((option) => option.name).sort()).toEqual(serverStates.map((state) => state.name).sort());
    expect(states.every((option) => !option.checked)).toBe(true);

    // In-panel search narrows the options in every facet at once.
    await driver.spaceFiltersSearchPanel("hig");
    expect((await driver.spaceFiltersPriorityOptions()).map((option) => option.name)).toEqual(["High"]);
    expect(await driver.spaceFiltersStateOptions()).toEqual([]);
    expect(await driver.spaceFiltersNoMatchesVisible()).toBe(true);

    // Clearing it restores the full option lists.
    await driver.spaceFiltersSearchPanel("");
    expect((await driver.spaceFiltersPriorityOptions()).map((option) => option.name)).toEqual([
      "Urgent",
      "High",
      "Medium",
      "Low",
      "None",
    ]);
    expect((await driver.spaceFiltersStateOptions()).map((option) => option.name).sort()).toEqual(
      serverStates.map((state) => state.name).sort()
    );
    expect(await driver.spaceFiltersNoMatchesVisible()).toBe(false);

    // Option search never touches the board itself (see SPC-039).
    await expectBoardNames(driver, seed.issueNames);
  }
);

test(
  specTitle(["SPC-035"], "applied pills support per-value, per-facet and clear-all removal, synced to the address"),
  { tag: specTags(["SPC-035"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const probe = `${ISSUE_PREFIX}filter`;
    await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, { name: probe, priority: "high" });

    await driver.spaceOpenBoard(anchor, "?board=list");
    await expectBoardNames(driver, [...seed.issueNames, probe]);

    await driver.spaceFiltersOpenPanel();
    await driver.spaceFiltersBeginFetchWatch();
    await driver.spaceFiltersPickPriority("High");
    await driver.spaceFiltersPickPriority("Urgent");
    await expect
      .poll(() => driver.spaceFiltersAppliedFacets(), BOARD_POLL)
      .toEqual([{ facet: "priority", values: ["high", "urgent"] }]);

    // Every change rewrites the address…
    expect(new URL(driver.page.url()).searchParams.get("priority")?.split(",").sort()).toEqual(["high", "urgent"]);
    // …narrows the board to the matching probe (no urgent issue exists)…
    await expectBoardNames(driver, [probe]);
    // …exactly like the server narrows for the same parameters…
    expect((await serverSpaceIssues(anchor, { priority: "high,urgent" })).map((issue) => issue.name)).toEqual([probe]);
    // …and refetches page one with the filters attached.
    const queries = await driver.spaceFiltersFetchQueries();
    const last = new URLSearchParams(queries[queries.length - 1] ?? "");
    expect(last.get("priority")?.split(",").sort()).toEqual(["high", "urgent"]);
    expect(last.get("cursor")).toBe("50:0:0");
    await driver.spaceFiltersEndFetchWatch();

    // Per-value removal drops one pill and keeps the rest.
    await driver.spaceFiltersRemoveValue("priority", "high");
    expect(await driver.spaceFiltersAppliedFacets()).toEqual([{ facet: "priority", values: ["urgent"] }]);
    expect(new URL(driver.page.url()).searchParams.get("priority")).toBe("urgent");
    await expectBoardNames(driver, []);

    // Per-facet removal clears the whole facet.
    await driver.spaceFiltersPickPriority("High");
    await expect
      .poll(() => driver.spaceFiltersAppliedFacets(), BOARD_POLL)
      .toEqual([{ facet: "priority", values: ["urgent", "high"] }]);
    await driver.spaceFiltersClearFacet("priority");
    expect(await driver.spaceFiltersAppliedStripVisible()).toBe(false);
    expect(new URL(driver.page.url()).searchParams.has("priority")).toBe(false);
    await expectBoardNames(driver, [...seed.issueNames, probe]);

    // Clear-all removes every facet at once.
    await driver.spaceFiltersPickPriority("High");
    await driver.spaceFiltersPickPriority("Urgent");
    await expect
      .poll(() => driver.spaceFiltersAppliedFacets(), BOARD_POLL)
      .toEqual([{ facet: "priority", values: ["high", "urgent"] }]);
    await driver.spaceFiltersClearAll();
    expect(await driver.spaceFiltersAppliedStripVisible()).toBe(false);
    const cleared = new URL(driver.page.url());
    expect(cleared.searchParams.get("board")).toBe("list");
    expect(cleared.searchParams.has("priority")).toBe(false);
    await expectBoardNames(driver, [...seed.issueNames, probe]);
  }
);

test(
  specTitle(["SPC-035"], "bug: strip removal of a state value writes an unreadable states parameter"),
  { tag: specTags(["SPC-035"]) },
  async ({ driver, seed }) => {
    // Intended: removal rewrites under the readable singular state key, so
    // the address keeps reproducing the board. See NEWFRONT-464.
    const anchor = requireAnchor(seed);
    // Two applied state values are needed: the seeded Todo plus a probe.
    const extra = await createState(seed.workspaceSlug, seed.projectId, session, `${STATE_PREFIX}started`);
    try {
      const states = await serverSpaceStates(anchor);
      const todo = states.find((state) => state.name === "Todo");
      if (!todo) throw new Error("[parity] seeded Todo state missing from the public states.");
      const extraPublic = states.find((state) => state.id === extra.id);
      if (!extraPublic) throw new Error("[parity] probe state missing from the public states.");

      await driver.spaceOpenBoard(anchor, "?board=list");
      await expectBoardNames(driver, seed.issueNames);

      await driver.spaceFiltersOpenPanel();
      await driver.spaceFiltersPickState("Todo");
      await driver.spaceFiltersPickState(extra.name);
      await expect
        .poll(() => driver.spaceFiltersAppliedFacets(), BOARD_POLL)
        .toEqual([{ facet: "state", values: ["Todo", extra.name] }]);
      expect(new URL(driver.page.url()).searchParams.get("state")?.split(",").sort()).toEqual(
        [todo.id, extra.id].sort()
      );

      if (driver.target === "web_new") {
        // Intended behavior (NEWFRONT-464): the survivor rewrites under the
        // singular state key, so the address keeps reproducing the board.
        await driver.spaceFiltersRemoveValue("state", "Todo");
        const next = new URL(driver.page.url());
        expect(next.searchParams.get("state")).toBe(extra.id);
        expect(next.searchParams.has("states")).toBe(false);
        await expectBoardNames(driver, []);
        await driver.page.reload();
        await expectBoardNames(driver, []);
        await expect
          .poll(() => driver.spaceFiltersAppliedFacets(), BOARD_POLL)
          .toEqual([{ facet: "state", values: [extra.name] }]);
        return;
      }
      // Removing one pill keeps the store correct but rewrites the survivor
      // under states= (plural), a key no reader understands.
      await driver.spaceFiltersRemoveValue("state", "Todo");
      const url = new URL(driver.page.url());
      expect(url.searchParams.get("states")).toBe(extra.id);
      expect(url.searchParams.has("state")).toBe(false);

      // The live board still filters from the store (nothing sits in the
      // probe state)…
      await expectBoardNames(driver, []);
      // …but the address no longer reproduces it: reload drops the filter.
      await driver.page.reload();
      await expectBoardNames(driver, seed.issueNames);
      expect(await driver.spaceFiltersAppliedStripVisible()).toBe(false);
    } finally {
      await deleteState(seed.workspaceSlug, seed.projectId, extra.id, session);
    }
  }
);

test(
  specTitle(["SPC-036"], "bug: pasted filter address seeds pills but loads the list unfiltered"),
  { tag: specTags(["SPC-036"]) },
  async ({ driver, seed }) => {
    // Intended: pasting an address with state, priority and label
    // parameters reproduces the same filtered board (the initial fetch
    // carries the parameters). The old app seeds the pills from the
    // address but reads the filter store before the address lands in it,
    // so its initial fetch goes out unfiltered and nothing refetches after
    // the seed. See NEWFRONT-463. web_new reads the address into the first
    // request, so the row is only green on the new leg with the intended
    // assertions.
    const anchor = requireAnchor(seed);
    const probe = `${ISSUE_PREFIX}shared`;
    await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, { name: probe, priority: "high" });
    const states = await serverSpaceStates(anchor);
    const todo = states.find((state) => state.name === "Todo");
    if (!todo) throw new Error("[parity] seeded Todo state missing from the public states.");
    const wantedPills = [
      { facet: "priority", values: ["high"] },
      { facet: "state", values: ["Todo"] },
    ];

    // The server narrows for these parameters; whether the list does depends
    // on which app built the request.
    expect((await serverSpaceIssues(anchor, { state: todo.id, priority: "high" })).map((issue) => issue.name)).toEqual([
      probe,
    ]);

    await driver.spaceFiltersBeginFetchWatch();
    await driver.spaceOpenBoard(anchor, `?board=list&state=${todo.id}&priority=high`);
    if (driver.target === "web_new") {
      // Intended: the address's facets ride the first list fetch, so the
      // board opens already narrowed and the pills describe it.
      await expectBoardNames(driver, [probe]);
      expect(await driver.spaceFiltersAppliedFacets()).toEqual(wantedPills);
      const landed = await driver.spaceFiltersFetchQueries();
      expect(landed.length).toBeGreaterThan(0);
      for (const query of landed) {
        const params = new URLSearchParams(query);
        expect(params.get("state")).toBe(todo.id);
        expect(params.get("priority")).toBe("high");
      }
      await driver.spaceFiltersEndFetchWatch();

      // Reload reads the same address the same way.
      await driver.page.reload();
      await expectBoardNames(driver, [probe]);
      expect(await driver.spaceFiltersAppliedFacets()).toEqual(wantedPills);

      // Clearing strips the facet keys, keeps the view, and widens the board.
      await driver.spaceFiltersClearAll();
      const cleared = new URL(driver.page.url());
      expect(cleared.searchParams.get("board")).toBe("list");
      expect(cleared.searchParams.has("priority")).toBe(false);
      expect(cleared.searchParams.has("state")).toBe(false);
      expect(cleared.searchParams.has("labels")).toBe(false);
      await expectBoardNames(driver, [...seed.issueNames, probe]);
      return;
    }

    // Observed old behavior: the pills seed from the address…
    await expectBoardNames(driver, [...seed.issueNames, probe]);
    expect(await driver.spaceFiltersAppliedFacets()).toEqual(wantedPills);
    // …but the list stays unfiltered: no initial fetch carries the parameters.
    const queries = await driver.spaceFiltersFetchQueries();
    expect(queries.length).toBeGreaterThan(0);
    for (const query of queries) {
      const params = new URLSearchParams(query);
      expect(params.has("priority")).toBe(false);
      expect(params.has("state")).toBe(false);
    }
    await driver.spaceFiltersEndFetchWatch();

    // Reload reproduces the same state: pills plus the unfiltered list.
    await driver.page.reload();
    await expectBoardNames(driver, [...seed.issueNames, probe]);
    expect(await driver.spaceFiltersAppliedFacets()).toEqual([
      { facet: "priority", values: ["high"] },
      { facet: "state", values: ["Todo"] },
    ]);

    // Clearing still strips the filter parameters while keeping the view.
    await driver.spaceFiltersClearAll();
    const cleared = new URL(driver.page.url());
    expect(cleared.searchParams.get("board")).toBe("list");
    expect(cleared.searchParams.has("priority")).toBe(false);
    expect(cleared.searchParams.has("state")).toBe(false);
    expect(cleared.searchParams.has("labels")).toBe(false);
    await expectBoardNames(driver, [...seed.issueNames, probe]);
  }
);

test(
  specTitle(["SPC-037"], "bug: label-bearing address shows the shell but never narrows the list"),
  { tag: specTags(["SPC-037"]) },
  async ({ driver, seed }) => {
    // Intended: a label-bearing address narrows the list fetch server-side
    // (the initial fetch and every page carry the label parameter). The old
    // app renders the bare shell but its load fetch drops the parameter —
    // the same dropped-seed root cause as SPC-036 (NEWFRONT-463). web_new
    // sends it, so the row is only green on the new leg with the intended
    // assertions.
    const anchor = requireAnchor(seed);
    const label = await createProjectLabel(seed.workspaceSlug, seed.projectId, session, `${LABEL_PREFIX}one`);
    const probe = `${ISSUE_PREFIX}labeled`;
    await serverSpaceCreateIssue(seed.workspaceSlug, seed.projectId, session, { name: probe, labelIds: [label.id] });

    // The server honors the parameter; whether the list does depends on
    // which app built the request.
    expect((await serverSpaceIssues(anchor, { labels: label.id })).map((issue) => issue.name)).toEqual([probe]);

    await driver.spaceFiltersBeginFetchWatch();
    await driver.spaceOpenBoard(anchor, `?board=list&labels=${label.id}`);
    if (driver.target === "web_new") {
      // Intended: the label narrows the first load, and the strip still
      // shows a bare shell since the panel offers no label options.
      await expectBoardNames(driver, [probe]);
      expect(await driver.spaceFiltersAppliedFacets()).toEqual([{ facet: "labels", values: [] }]);
      const landed = await driver.spaceFiltersFetchQueries();
      expect(landed.length).toBeGreaterThan(0);
      for (const query of landed) {
        expect(new URLSearchParams(query).get("labels")).toBe(label.id);
      }
      await driver.spaceFiltersEndFetchWatch();

      // The shell's facet-level remove clears it like any other facet and
      // widens the board back to everything.
      await driver.spaceFiltersClearFacet("labels");
      expect(new URL(driver.page.url()).searchParams.has("labels")).toBe(false);
      await expectBoardNames(driver, [...seed.issueNames, probe]);

      // Pasting the label address again narrows it again.
      await driver.spaceOpenBoard(anchor, `?board=list&labels=${label.id}`);
      await expectBoardNames(driver, [probe]);

      // The panel offers no label options to change it with.
      await driver.spaceFiltersOpenPanel();
      expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);
      return;
    }

    // Observed old behavior: the bare shell renders…
    await expectBoardNames(driver, [...seed.issueNames, probe]);
    expect(await driver.spaceFiltersAppliedFacets()).toEqual([{ facet: "labels", values: [] }]);
    // …but no fetch carries the label parameter.
    const queries = await driver.spaceFiltersFetchQueries();
    expect(queries.length).toBeGreaterThan(0);
    for (const query of queries) {
      expect(new URLSearchParams(query).has("labels")).toBe(false);
    }
    await driver.spaceFiltersEndFetchWatch();

    // The shell's facet-level remove clears it like any other facet.
    await driver.spaceFiltersClearFacet("labels");
    expect(new URL(driver.page.url()).searchParams.has("labels")).toBe(false);
    await expectBoardNames(driver, [...seed.issueNames, probe]);

    // The panel offers no label options to change it with.
    await driver.spaceOpenBoard(anchor, `?board=list&labels=${label.id}`);
    await expectBoardNames(driver, [...seed.issueNames, probe]);
    await driver.spaceFiltersOpenPanel();
    expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);
  }
);

test(
  specTitle(["SPC-038"], "filter facets follow the active layout's supported set"),
  { tag: specTags(["SPC-038"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    expect((await serverSpaceSettings(anchor)).views).toMatchObject({ list: true, kanban: true });

    await driver.spaceOpenBoard(anchor, "?board=list");
    await expectBoardNames(driver, seed.issueNames);
    await driver.spaceFiltersOpenPanel();
    expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);

    // Both offered layouts support the same set, so switching keeps it.
    await driver.spaceFiltersSwitchLayout("kanban");
    await driver.spaceFiltersOpenPanel();
    expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);

    await driver.spaceFiltersSwitchLayout("list");
    await driver.spaceFiltersOpenPanel();
    expect(await driver.spaceFiltersFacetNames()).toEqual(["Priority", "State"]);
  }
);

test(
  specTitle(["SPC-039"], "no free-text issue search exists; panel search narrows options only"),
  { tag: specTags(["SPC-039"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await driver.spaceOpenBoard(anchor, "?board=list");
    await expectBoardNames(driver, seed.issueNames);

    // No search box anywhere on the board while the panel is closed…
    expect(await driver.spaceFiltersSearchBoxCount()).toBe(0);

    await driver.spaceFiltersBeginFetchWatch();
    await driver.spaceFiltersOpenPanel();
    // …and exactly one with it open: the panel's own option search.
    expect(await driver.spaceFiltersSearchBoxCount()).toBe(1);

    // Typing an issue title narrows the facet options, never the list…
    await driver.spaceFiltersSearchPanel("Parity second");
    expect((await driver.spaceFiltersPriorityOptions()).map((option) => option.name)).toEqual([]);
    expect(await driver.spaceFiltersStateOptions()).toEqual([]);
    expect(await driver.spaceFiltersNoMatchesVisible()).toBe(true);

    // …the board still shows every issue and no refetch fires.
    await expectBoardNames(driver, seed.issueNames);
    await driver.page.waitForTimeout(2000);
    expect(await driver.spaceFiltersFetchQueries()).toEqual([]);
    await driver.spaceFiltersEndFetchWatch();
  }
);
