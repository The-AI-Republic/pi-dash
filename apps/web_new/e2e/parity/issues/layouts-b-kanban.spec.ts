// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-118): Issues board / kanban layout — grouped
// columns, swimlanes, collapse, cards, quick-add, header create, every
// drag-drop move including the delete zone and feedback overlays, plus
// per-column pagination and virtualization. Rows: ISS-028..ISS-043.
// Precondition: a fresh seeded stack (parity-up.sh); every scenario sets
// its own layout preferences through the API, enters the app
// pre-authenticated, and deletes the rows it creates so later scenarios
// see the seed again.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/index";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserCookies,
  serverAddIssuesToCycle,
  serverCreateCycle,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateModule,
  serverCreateProject,
  serverCreateState,
  serverDeleteCycle,
  serverDeleteIssue,
  serverDeleteLabel,
  serverDeleteProject,
  serverDeleteState,
  serverIssueDetails,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProject,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  serverRemoveIssueFromCycle,
  signInFreshUser,
  uniqueSuffix,
  type FreshUser,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** An opened board plus the prefs it started with, for teardown restore. */
interface BoardContext {
  user: FreshUser;
  beforeFilters: Record<string, unknown>;
  beforeProperties: Record<string, unknown>;
}

/**
 * Set a project's board preferences through the API, then enter the app
 * pre-authenticated and wait for the board to render. Manual order and
 * visible empty groups are the default; callers override per scenario.
 */
async function openBoard(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  filters: Record<string, unknown>,
  properties?: Record<string, unknown>
): Promise<BoardContext> {
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      sub_group_by: null,
      show_empty_groups: true,
      ...filters,
    },
    display_properties: { ...before.displayProperties, ...properties },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(user));
  await kanbanOpenWithRetry(driver);
  return { user, beforeFilters: before.displayFilters, beforeProperties: before.displayProperties };
}

/**
 * Open the board, retrying once through a reload. Under host contention
 * the issues page occasionally never renders its switcher inside the
 * driver's long wait (the same load stall the timeline helper retries).
 * The retry only absorbs that stall: a genuinely broken board still
 * fails its second wait.
 */
async function kanbanOpenWithRetry(driver: ParityDriver): Promise<void> {
  try {
    await driver.kanbanOpenBoard();
  } catch {
    await driver.boardReloadIssues();
    await driver.kanbanOpenBoard();
  }
}

/**
 * Reload the issues page, retrying once when the reload itself stalls.
 * Under host contention the reloaded page occasionally never renders its
 * switcher inside the driver's long wait; the post-rebase re-verify lost
 * five attempts to that single unguarded reload. The retry only absorbs
 * the stall: a genuinely wedged page still fails its second wait.
 */
async function boardReloadWithRetry(driver: ParityDriver): Promise<void> {
  try {
    await driver.boardReloadIssues();
  } catch {
    await driver.boardReloadIssues();
  }
}

/** Reload the issues page and wait for the board, absorbing one load stall at each step. */
async function kanbanReloadWithRetry(driver: ParityDriver): Promise<void> {
  await boardReloadWithRetry(driver);
  await kanbanOpenWithRetry(driver);
}

/** Restore the exact preferences an opened board started with. */
async function restoreBoard(seed: ParitySeedFacts, projectId: string, ctx: BoardContext): Promise<void> {
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
    display_filters: ctx.beforeFilters,
    display_properties: ctx.beforeProperties,
  });
}

/** Switch one board preference and reload so the board re-renders. */
async function setBoardFilters(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  projectId: string,
  ctx: BoardContext,
  filters: Record<string, unknown>
): Promise<void> {
  const current = await serverProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
    display_filters: { ...current.displayFilters, ...filters },
  });
  await kanbanReloadWithRetry(driver);
}

/** Server-side UUID of an issue looked up by its name. */
async function issueIdByName(seed: ParitySeedFacts, projectId: string, cookie: string, name: string): Promise<string> {
  const rows = await serverIssues(seed.workspaceSlug, projectId, cookie);
  const found = rows.find((row) => row.name === name);
  if (!found) throw new Error(`[parity] no server issue named ${JSON.stringify(name)}.`);
  return found.id;
}

test(
  specTitle(["ISS-028"], "kanban grouped columns, None column and ungrouped single column"),
  { tag: specTags(["ISS-028"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(
      seed.workspaceSlug,
      owner.cookie,
      `KB columns ${suffix}`,
      `KB${suffix}`
    );
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const fallback = states.find((state) => state.isDefault) ?? states[0];
    if (!fallback) throw new Error("[parity] scratch project has no states.");
    const other = states.find((state) => state.id !== fallback.id);
    const secondName = `KB second ${suffix}`;
    const thirdName = `KB third ${suffix}`;
    const firstName = `KB first ${suffix}`;
    const firstId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, firstName, fallback.id);
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, secondName, fallback.id);
    const thirdId = await serverCreateIssue(
      seed.workspaceSlug,
      projectId,
      owner.cookie,
      thirdName,
      other?.id ?? fallback.id
    );
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `KB label ${suffix}`, "#666666", owner.cookie);
    await serverPatchIssue(seed.workspaceSlug, projectId, firstId, { label_ids: [label.id] }, owner.cookie);
    const ctx = await openBoard(driver, seed, projectId, { group_by: "state" });

    await test.step("one column per state with live counts", async () => {
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(states.length);
      const columns = await driver.kanbanColumns();
      for (const state of states) {
        const column = columns.find((entry) => entry.name === state.name);
        expect(column, `column for state ${state.name}`).toBeDefined();
      }
      const home = columns.find((entry) => entry.name === fallback.name);
      expect(home?.count).toBe(other ? 2 : 3);
      if (other) {
        expect(columns.find((entry) => entry.name === other.name)?.count).toBe(1);
      }
      const server = await serverIssues(seed.workspaceSlug, projectId, owner.cookie);
      expect(server).toHaveLength(3);
    });

    await test.step("labels group into the value plus a synthetic None", async () => {
      await setBoardFilters(driver, seed, projectId, ctx, { group_by: "labels" });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
      const columns = await driver.kanbanColumns();
      expect(columns.find((entry) => entry.name === label.name)?.count).toBe(1);
      const none = columns.find((entry) => entry.name !== label.name);
      expect(none?.name).toBe("None");
      expect(none?.count).toBe(2);
      expect(await driver.kanbanColumnCards(label.name)).toEqual([firstName]);
    });

    await test.step("ungrouped boards collapse to a single column", async () => {
      await setBoardFilters(driver, seed, projectId, ctx, { group_by: null });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
      const columns = await driver.kanbanColumns();
      expect(columns[0]?.count).toBe(3);
      expect(await driver.kanbanColumnCards(columns[0]?.name ?? "")).toHaveLength(3);
    });

    await test.step("priority grouping offers one column per value", async () => {
      await serverPatchIssue(seed.workspaceSlug, projectId, firstId, { priority: "high" }, owner.cookie);
      const secondId = await issueIdByName(seed, projectId, owner.cookie, secondName);
      await serverPatchIssue(seed.workspaceSlug, projectId, secondId, { priority: "medium" }, owner.cookie);
      await serverPatchIssue(seed.workspaceSlug, projectId, thirdId, { priority: "low" }, owner.cookie);
      await setBoardFilters(driver, seed, projectId, ctx, { group_by: "priority" });
      // Empty groups stay visible, so all five priority columns render.
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(5);
      const columns = await driver.kanbanColumns();
      expect(columns.find((entry) => entry.name === "High")?.count).toBe(1);
      expect(columns.find((entry) => entry.name === "Medium")?.count).toBe(1);
      expect(columns.find((entry) => entry.name === "Low")?.count).toBe(1);
      expect(columns.find((entry) => entry.name === "Urgent")?.count).toBe(0);
      expect(columns.find((entry) => entry.name === "None")?.count).toBe(0);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-028"], "bug: grouped column count excludes the pending-triage issue (NEWFRONT-153)"),
  { tag: specTags(["ISS-028"]) },
  async ({ driver, seed }) => {
    // The old backend's grouped count filter drops the pending-triage seed
    // issue while the grouped results keep it, so the Todo header undercounts
    // the cards it renders. The new app implements the intended behavior
    // (count equals cards); this scenario pins the old one until NEWFRONT-153
    // lands.
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });

    await test.step("the header undercounts the cards it renders", async () => {
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
      const columns = await driver.kanbanColumns();
      const cards = await driver.kanbanColumnCards(columns[0]?.name ?? "");
      expect(cards).toHaveLength(3);
      expect(columns[0]?.count).toBeLessThan(cards.length);
    });

    await test.step("the server holds three issues in that state", async () => {
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, ctx.user.cookie);
      expect(rows).toHaveLength(3);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-029"], "kanban swimlanes with cumulative counts"),
  { tag: specTags(["ISS-029"]) },
  async ({ driver, seed }) => {
    // A scratch project keeps the lane counts exact: the seed project's
    // grouped counts undercount (NEWFRONT-153).
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB lanes ${suffix}`, `KL${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `KB lane ${suffix}`, "#666666", owner.cookie);
    const firstId = await serverCreateIssue(
      seed.workspaceSlug,
      projectId,
      owner.cookie,
      `KB lane a ${suffix}`,
      home.id
    );
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB lane b ${suffix}`, home.id);
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB lane c ${suffix}`, home.id);
    await serverPatchIssue(seed.workspaceSlug, projectId, firstId, { label_ids: [label.id] }, owner.cookie);
    const ctx = await openBoard(driver, seed, projectId, { group_by: "state", sub_group_by: "labels" });

    await test.step("one lane per sub-group value with cumulative counts", async () => {
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);
      const lanes = await driver.kanbanSwimlanes();
      expect(lanes.find((entry) => entry.name === label.name)?.count).toBe(1);
      const none = lanes.find((entry) => entry.name !== label.name);
      expect(none?.name).toBe("None");
      expect(none?.count).toBe(2);
      const groups = await driver.kanbanColumns();
      expect(groups).toHaveLength(states.length);
    });

    await test.step("empty lanes hide when show-empty is off", async () => {
      await serverCreateLabel(seed.workspaceSlug, projectId, `KB empty ${suffix}`, "#666666", owner.cookie);
      await kanbanReloadWithRetry(driver);
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(3);
      await setBoardFilters(driver, seed, projectId, ctx, { show_empty_groups: false });
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);
      const lanes = await driver.kanbanSwimlanes();
      expect(lanes.map((entry) => entry.name)).not.toContain(`KB empty ${suffix}`);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-030"], "collapse and expand a kanban group column"),
  { tag: specTags(["ISS-030"]) },
  async ({ driver, seed }) => {
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
    const name = (await driver.kanbanColumns())[0]?.name ?? "";
    const countBefore = (await driver.kanbanColumns())[0]?.count ?? 0;

    await test.step("collapsing folds the column but keeps its count", async () => {
      expect(await driver.kanbanColumnCards(name)).toHaveLength(3);
      await driver.kanbanToggleColumn(name);
      expect(await driver.kanbanColumnCollapsed(name)).toBe(true);
      expect(await driver.kanbanColumnCards(name)).toHaveLength(0);
      expect((await driver.kanbanColumns())[0]?.count).toBe(countBefore);
    });

    await test.step("the collapsed state survives a reload", async () => {
      await kanbanReloadWithRetry(driver);
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
      expect(await driver.kanbanColumnCollapsed(name)).toBe(true);
      await driver.kanbanToggleColumn(name);
      expect(await driver.kanbanColumnCollapsed(name)).toBe(false);
      expect(await driver.kanbanColumnCards(name)).toHaveLength(3);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-031"], "collapse and expand a kanban swimlane"),
  { tag: specTags(["ISS-031"]) },
  async ({ driver, seed }) => {
    // A scratch project keeps the lanes exact: the seed's unlabeled rows
    // never reach a labels sub-grouped board (NEWFRONT-158); same
    // workaround as ISS-029 for NEWFRONT-153.
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB fold ${suffix}`, `KF${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `KB fold ${suffix}`, "#666666", owner.cookie);
    const firstName = `KB fold a ${suffix}`;
    const firstId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, firstName, home.id);
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB fold b ${suffix}`, home.id);
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB fold c ${suffix}`, home.id);
    await serverPatchIssue(seed.workspaceSlug, projectId, firstId, { label_ids: [label.id] }, owner.cookie);
    await openBoard(driver, seed, projectId, { group_by: "state", sub_group_by: "labels" });
    await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);

    await test.step("collapsing a lane hides its cards", async () => {
      expect(await driver.kanbanSwimlaneCollapsed(label.name)).toBe(false);
      await driver.kanbanToggleSwimlane(label.name);
      expect(await driver.kanbanSwimlaneCollapsed(label.name)).toBe(true);
      // Lane columns load lazily: the toggle only waits for the collapse
      // flag, so poll until the surviving lane's cards mount.
      await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(2);
      const cards = await driver.kanbanCards();
      expect(cards.map((card) => card.name)).not.toContain(firstName);
    });

    await test.step("the lane state survives a reload", async () => {
      await kanbanReloadWithRetry(driver);
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);
      expect(await driver.kanbanSwimlaneCollapsed(label.name)).toBe(true);
      await driver.kanbanToggleSwimlane(label.name);
      expect(await driver.kanbanSwimlaneCollapsed(label.name)).toBe(false);
      await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-031"], "bug: labels sub-grouping drops unlabeled seed issues (NEWFRONT-158)"),
  { tag: specTags(["ISS-031"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6);
    const label = await serverCreateLabel(
      seed.workspaceSlug,
      seed.projectId,
      `KB fold ${suffix}`,
      "#666666",
      owner.cookie
    );
    const firstId = await issueIdByName(seed, seed.projectId, owner.cookie, seed.issueNames[0] ?? "");
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { label_ids: [label.id] }, owner.cookie);
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state", sub_group_by: "labels" });
    // Cleanup runs even when an assertion fails: a leaked KB fold label
    // self-cascades into the retry (test-3: the retry died on attempt 1's
    // label because the cleanup step never ran).
    try {
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);

      await test.step("the None lane stays empty on the seed project", async () => {
        // Intended: the two unlabeled seed issues render here (count 2).
        // Pinned: the labels sub-grouped query drops seed rows without
        // labels, so the lane shows count 0 and no cards (NEWFRONT-158).
        // The first render can briefly show the flat-grouped rows (None at
        // count 2 with the seed cards) before the grouped response replaces
        // them — the post-rebase re-verify read exactly that transient — so
        // poll until the lane settles instead of reading it once.
        await expect
          .poll(async () => (await driver.kanbanCards()).map((card) => card.name), { timeout: 120_000 })
          .toContain(seed.issueNames[0] ?? "");
        const laneSettled = async () => {
          const lanes = await driver.kanbanSwimlanes();
          const names = (await driver.kanbanCards()).map((card) => card.name);
          return {
            none: lanes.find((entry) => entry.name === "None")?.count,
            hasFirst: names.includes(seed.issueNames[0] ?? ""),
            hasSecond: names.includes(seed.issueNames[1] ?? ""),
            hasThird: names.includes(seed.issueNames[2] ?? ""),
          };
        };
        const settled = { none: 0, hasFirst: true, hasSecond: false, hasThird: false };
        try {
          await expect.poll(laneSettled, { timeout: 120_000 }).toEqual(settled);
        } catch {
          // The grouped fetch itself can stall under contention: test-3
          // held the flat pre-grouped render (None at 2) for the whole
          // 120s window while the server deterministically drops the rows
          // (10/10 direct reads). One reload re-issues the fetch; a
          // genuinely changed behavior still fails its second window.
          await kanbanReloadWithRetry(driver);
          await expect.poll(laneSettled, { timeout: 120_000 }).toEqual(settled);
        }
        const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
        expect(rows).toHaveLength(3);
      });
    } finally {
      await test.step("cleanup removes the label and restores preferences", async () => {
        await serverPatchIssue(seed.workspaceSlug, seed.projectId, firstId, { label_ids: [] }, owner.cookie);
        await serverDeleteLabel(seed.workspaceSlug, seed.projectId, label.id, owner.cookie);
        await restoreBoard(seed, seed.projectId, ctx);
      });
    }
  }
);

test(
  specTitle(["ISS-032"], "show and hide empty kanban groups"),
  { tag: specTags(["ISS-032"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6);
    const emptyName = `KB void ${suffix}`;
    const emptyId = await serverCreateState(seed.workspaceSlug, seed.projectId, emptyName, "unstarted", owner.cookie);
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });

    await test.step("empty columns show by default and hide when show-empty is off", async () => {
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
      await setBoardFilters(driver, seed, seed.projectId, ctx, { show_empty_groups: false });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
      const columns = await driver.kanbanColumns();
      expect(columns.map((entry) => entry.name)).not.toContain(emptyName);
    });

    await test.step("cleanup removes the state and restores preferences", async () => {
      await serverDeleteState(seed.workspaceSlug, seed.projectId, emptyId, owner.cookie);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(specTitle(["ISS-033"], "kanban card contents"), { tag: specTags(["ISS-033"]) }, async ({ driver, seed }) => {
  const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" }, { key: true, priority: true });
  const name = seed.issueNames[0] ?? "";
  await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);

  await test.step("cards show the identifier and wrapped properties", async () => {
    const id = await issueIdByName(seed, seed.projectId, ctx.user.cookie, name);
    const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, ctx.user.cookie);
    expect(await driver.kanbanCardIdentifier(name)).toBe(`PAR-${details.sequenceId}`);
    expect(await driver.kanbanCardShowsProperties(name)).toBe(true);
  });

  await test.step("hovering reveals the card quick-actions", async () => {
    await driver.kanbanCardHover(name);
    await expect.poll(() => driver.kanbanCardQuickActionsVisible(name), { timeout: 30_000 }).toBe(true);
  });

  await test.step("cleanup restores the seed preferences", async () => {
    await restoreBoard(seed, seed.projectId, ctx);
  });
});

test(
  specTitle(["ISS-034"], "open an issue from a kanban card"),
  { tag: specTags(["ISS-034"]) },
  async ({ driver, seed }) => {
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    const name = seed.issueNames[1] ?? "";
    await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);

    await test.step("cards are real anchors to the work item", async () => {
      const id = await issueIdByName(seed, seed.projectId, ctx.user.cookie, name);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, ctx.user.cookie);
      const href = await driver.kanbanCardHref(name);
      expect(href).not.toBeNull();
      expect(href ?? "").toContain(`PAR-${details.sequenceId}`);
    });

    await test.step("clicking opens peek on desktop", async () => {
      await driver.kanbanOpenCardPeek(name);
      expect(await driver.issuePeekVisible()).toBe(true);
      expect(await driver.issuePeekTitle()).toBe(name);
      await driver.issuePeekClose();
      expect(await driver.issuePeekVisible()).toBe(false);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-035"], "per-group quick-add on the kanban board"),
  { tag: specTags(["ISS-035"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const states = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
    const fallback = states.find((state) => state.isDefault) ?? states[0];
    if (!fallback) throw new Error("[parity] seed project has no states.");
    const doneId = await serverCreateState(
      seed.workspaceSlug,
      seed.projectId,
      `Done ${uniqueSuffix().slice(0, 6)}`,
      "completed",
      owner.cookie
    );
    const statesAfter = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
    const done = statesAfter.find((state) => state.id === doneId);
    if (!done) throw new Error("[parity] created state never listed.");
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
    const suffix = uniqueSuffix().slice(0, 6);
    const title = `KB quick ${suffix}`;

    await test.step("quick-add lands in the column group with the default state", async () => {
      expect(await driver.kanbanColumnHasQuickAdd(done.name)).toBe(true);
      await driver.kanbanQuickAdd(done.name, title);
      expect(await driver.kanbanColumnCards(done.name)).toContain(title);
      const id = await issueIdByName(seed, seed.projectId, owner.cookie, title);
      const details = await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie);
      // The column group wins over the seeded default: the card lands Done.
      expect(details.stateId).toBe(doneId);
      await serverDeleteIssue(seed.workspaceSlug, seed.projectId, id, owner.cookie);
    });

    await test.step("created-by columns still offer quick-add", async () => {
      // The inventory claims created-by columns never allow quick-add, but
      // the old app renders the entry there; the row records the correction
      // and the new app follows the app, not the row's first draft. Every
      // creator renders a column (empty groups stay visible), so assert the
      // entry on each of them rather than a fixed column count.
      await setBoardFilters(driver, seed, seed.projectId, ctx, { group_by: "created_by" });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).not.toHaveLength(0);
      for (const column of await driver.kanbanColumns()) {
        expect(await driver.kanbanColumnHasQuickAdd(column.name)).toBe(true);
      }
    });

    await test.step("guests see no quick-add entry", async () => {
      if (!seed.guestEmail || !seed.guestPassword) {
        throw new Error("[parity] seed carries no guest; re-run the stack seed step.");
      }
      // Guests see an empty board until the project lets them use everything;
      // even then the board stays view-only with no quick-add entry.
      await serverPatchProject(seed.workspaceSlug, seed.projectId, owner.cookie, { guest_view_all_features: true });
      try {
        const guest = await signInFreshUser(seed.guestEmail, seed.guestPassword);
        await driver.openAuthenticated(
          `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
          browserCookies(guest)
        );
        await kanbanOpenWithRetry(driver);
        await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).not.toHaveLength(0);
        for (const column of await driver.kanbanColumns()) {
          expect(await driver.kanbanColumnHasQuickAdd(column.name)).toBe(false);
        }
      } finally {
        await serverPatchProject(seed.workspaceSlug, seed.projectId, owner.cookie, { guest_view_all_features: false });
      }
    });

    await test.step("cleanup removes the state and restores preferences", async () => {
      await serverDeleteState(seed.workspaceSlug, seed.projectId, doneId, owner.cookie);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-036"], "kanban group-header create and cycle add menu"),
  { tag: specTags(["ISS-036"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6);
    // An active cycle: membership calls reject completed (past-dated) cycles.
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      `KB cycle ${suffix}`,
      "2026-10-20",
      "2026-11-20",
      owner.cookie
    );
    const firstId = await issueIdByName(seed, seed.projectId, owner.cookie, seed.issueNames[0] ?? "");
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycleId, [firstId], owner.cookie);
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
    const column = (await driver.kanbanColumns())[0]?.name ?? "";

    await test.step("project context opens the create modal", async () => {
      expect(await driver.kanbanHeaderCreateVisible(column)).toBe(true);
      await driver.kanbanHeaderCreate(column);
      expect(await driver.kanbanCreateModalVisible()).toBe(true);
    });

    await test.step("cycle context offers create and add-existing", async () => {
      // "Cycle context" is the cycle route, not a cycle grouping: the menu
      // renders only when the board opens with a cycle id in the path.
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/cycles/${cycleId}`,
        browserCookies(ctx.user)
      );
      await kanbanOpenWithRetry(driver);
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(1);
      const column = (await driver.kanbanColumns())[0]?.name ?? "";
      expect(await driver.kanbanHeaderCreateVisible(column)).toBe(true);
      await driver.kanbanHeaderCreate(column);
      const items = await driver.kanbanHeaderMenuItems(column);
      expect(items.some((entry) => /creat/i.test(entry))).toBe(true);
      expect(items.some((entry) => /exist/i.test(entry))).toBe(true);
      const create = items.find((entry) => /creat/i.test(entry)) ?? "";
      await driver.kanbanHeaderMenuChoose(column, create);
      await expect.poll(() => driver.kanbanCreateModalVisible(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("cleanup removes the cycle and restores preferences", async () => {
      // Remove the member first: deleting a non-empty cycle orphans its
      // members' cycle pointer, hiding them from cycle-grouped boards.
      await serverRemoveIssueFromCycle(seed.workspaceSlug, seed.projectId, cycleId, firstId, owner.cookie);
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, owner.cookie);
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-037"], "reorder a card within its column"),
  { tag: specTags(["ISS-037"]) },
  async ({ driver, seed }) => {
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
    const column = (await driver.kanbanColumns())[0]?.name ?? "";
    const [first, second, third] = await driver.kanbanColumnCards(column);
    if (!first || !second || !third) throw new Error("[parity] seed column never filled.");
    const orderOf = async (name: string): Promise<number> => {
      const id = await issueIdByName(seed, seed.projectId, ctx.user.cookie, name);
      return (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, ctx.user.cookie)).sortOrder;
    };
    const beforeFirst = await orderOf(first);

    await test.step("dropping above the first card moves it first", async () => {
      await driver.kanbanDragCardBefore(third, first);
      await expect.poll(() => driver.kanbanColumnCards(column), { timeout: 60_000 }).toEqual([third, first, second]);
      expect(await orderOf(third)).toBeLessThan(beforeFirst);
    });

    await test.step("dropping between cards takes the midpoint order", async () => {
      await driver.kanbanDragCardBefore(second, first);
      await expect.poll(() => driver.kanbanColumnCards(column), { timeout: 60_000 }).toEqual([third, second, first]);
      const middle = await orderOf(second);
      expect(middle).toBeGreaterThan(await orderOf(third));
      expect(middle).toBeLessThan(await orderOf(first));
    });

    await test.step("cleanup restores orders and preferences", async () => {
      const ids = await Promise.all(
        [first, second, third].map((name) => issueIdByName(seed, seed.projectId, ctx.user.cookie, name))
      );
      // Restore a stable manual order: the seed sorts by creation sequence.
      const ranks = [15000, 25000, 35000];
      for (const [index, id] of ids.entries()) {
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          id,
          { sort_order: ranks[index] ?? 0 },
          ctx.user.cookie
        );
      }
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-037", "ISS-041"], "bug: non-manual order persists the drop without feedback (NEWFRONT-154)"),
  { tag: specTags(["ISS-037", "ISS-041"]) },
  async ({ driver, seed }) => {
    // The inventory says a non-manual order overlays the current order and
    // suppresses the move; the old app shows the generic drop hint and
    // persists a new sort_order with no toast and no visible change. The new
    // app implements the intended (suppressed) behavior; this scenario pins
    // the old one until NEWFRONT-154 lands.
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state", order_by: "priority" });
    await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(3);
    const column = (await driver.kanbanColumns())[0]?.name ?? "";
    const placed = await driver.kanbanColumnCards(column);
    const [top] = placed;
    if (!top) throw new Error("[parity] seed column never filled.");
    const orderOf = async (name: string): Promise<number> => {
      const id = await issueIdByName(seed, seed.projectId, ctx.user.cookie, name);
      return (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, ctx.user.cookie)).sortOrder;
    };
    const before = await orderOf(top);

    await test.step("the hold shows the generic hint, not the current order", async () => {
      const { overlay } = await driver.kanbanDragHoldOverColumn(top, column);
      expect(overlay).toBe("Drop here to move the work item");
    });

    await test.step("the release persists server-side with no visible change", async () => {
      expect(await driver.kanbanColumnCards(column)).toEqual(placed);
      expect(await driver.boardLastToast()).toBeNull();
      expect(await orderOf(top)).not.toBe(before);
    });

    await test.step("cleanup restores orders and preferences", async () => {
      const ranks = [15000, 25000, 35000];
      for (const [index, name] of seed.issueNames.entries()) {
        const id = await issueIdByName(seed, seed.projectId, ctx.user.cookie, name);
        await serverPatchIssue(
          seed.workspaceSlug,
          seed.projectId,
          id,
          { sort_order: ranks[index] ?? 0 },
          ctx.user.cookie
        );
      }
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-038"], "move a card to another column"),
  { tag: specTags(["ISS-038"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6);
    const states = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] seed project has no states.");
    const doneId = await serverCreateState(
      seed.workspaceSlug,
      seed.projectId,
      `Done ${suffix}`,
      "completed",
      owner.cookie
    );
    const [alpha] = seed.issueNames;
    if (!alpha) throw new Error("[parity] seed names missing.");
    const statesAfter = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
    const doneName = statesAfter.find((state) => state.id === doneId)?.name ?? "";
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);

    await test.step("scalar fields move to the destination column", async () => {
      const id = await issueIdByName(seed, seed.projectId, owner.cookie, alpha);
      const sortBefore = (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).sortOrder;
      await driver.kanbanDragCardToColumnEnd(alpha, doneName);
      await expect.poll(() => driver.kanbanColumnCards(doneName), { timeout: 60_000 }).toContain(alpha);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).stateId).toBe(doneId);
      // Restore both fields: later seed scenarios assume the pristine order.
      await serverPatchIssue(
        seed.workspaceSlug,
        seed.projectId,
        id,
        { state_id: home.id, sort_order: sortBefore },
        owner.cookie
      );
    });

    await test.step("a disallowed grouping warns and moves nothing", async () => {
      await setBoardFilters(driver, seed, seed.projectId, ctx, { group_by: "state_detail.group" });
      // State groups render the whole seven-value domain, not just the
      // occupied ones; the three seed cards land in the Todo column.
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).not.toHaveLength(0);
      await expect.poll(() => driver.kanbanCards(), { timeout: 60_000 }).toHaveLength(3);
      const columns = await driver.kanbanColumns();
      let target = "";
      for (const entry of columns) {
        if (!(await driver.kanbanColumnCards(entry.name)).includes(alpha)) {
          target = entry.name;
          break;
        }
      }
      expect(target).not.toBe("");
      const id = await issueIdByName(seed, seed.projectId, owner.cookie, alpha);
      const stateBefore = (await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).stateId;
      // A refused drop never settles, so hold-and-release instead of the
      // settling drag, then assert the warning and the unchanged card.
      await driver.kanbanDragHoldOverColumn(alpha, target);
      await expect.poll(() => driver.boardLastToast(), { timeout: 30_000 }).not.toBeNull();
      const toast = await driver.boardLastToast();
      expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`).toMatch(/cannot move/i);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).stateId).toBe(
        stateBefore
      );
    });

    await test.step("cleanup removes every fixture and restores preferences", async () => {
      await serverDeleteState(seed.workspaceSlug, seed.projectId, doneId, owner.cookie);
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-038"], "membership moves across label, module and cycle columns"),
  { tag: specTags(["ISS-038"]) },
  async ({ driver, seed }) => {
    // A scratch project keeps None exact: labels-grouped boards drop
    // unlabeled seed rows entirely (NEWFRONT-158), so the None column
    // only renders for API-made issues.
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB member ${suffix}`, `KE${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    const label = await serverCreateLabel(
      seed.workspaceSlug,
      projectId,
      `KB member ${suffix}`,
      "#666666",
      owner.cookie
    );
    const memberName = `KB member ${suffix}`;
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      projectId,
      memberName,
      "2026-09-15",
      "2026-11-15",
      owner.cookie
    );
    const moduleId = await serverCreateModule(seed.workspaceSlug, projectId, memberName, owner.cookie);
    const alpha = `KB member a ${suffix}`;
    const beta = `KB member b ${suffix}`;
    const gamma = `KB member c ${suffix}`;
    const alphaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, alpha, home.id);
    const betaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, beta, home.id);
    const gammaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, gamma, home.id);
    const ctx = await openBoard(driver, seed, projectId, { group_by: "labels" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);

    await test.step("array fields add the destination and clear on None", async () => {
      const none = (await driver.kanbanColumns()).find((entry) => entry.name !== label.name)?.name ?? "";
      expect(none).not.toBe("");
      await driver.kanbanDragCardToColumnEnd(beta, label.name);
      await expect.poll(() => driver.kanbanColumnCards(label.name), { timeout: 60_000 }).toContain(beta);
      // The card renders optimistically; the PATCH can land after the UI
      // read (test-3 forensics: the label was present ~40min later). Poll
      // the server truth until it persists instead of reading it once.
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, projectId, betaId, owner.cookie)).labelIds, {
          timeout: 60_000,
        })
        .toContain(label.id);
      await driver.kanbanDragCardToColumnEnd(beta, none);
      await expect.poll(() => driver.kanbanColumnCards(none), { timeout: 60_000 }).toContain(beta);
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, projectId, betaId, owner.cookie)).labelIds, {
          timeout: 60_000,
        })
        .toHaveLength(0);
    });

    await test.step("module membership moves through the module call", async () => {
      await setBoardFilters(driver, seed, projectId, ctx, { group_by: "module" });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
      await driver.kanbanDragCardToColumnEnd(gamma, memberName);
      await expect.poll(() => driver.kanbanColumnCards(memberName), { timeout: 60_000 }).toContain(gamma);
      // Same optimistic-render race as the label step above (test-3
      // forensics: the module was present ~40min later) — poll until it
      // persists.
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, projectId, gammaId, owner.cookie)).moduleIds, {
          timeout: 60_000,
        })
        .toContain(moduleId);
    });

    await test.step("cycle membership moves through the cycle call", async () => {
      await setBoardFilters(driver, seed, projectId, ctx, { group_by: "cycle" });
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
      await driver.kanbanDragCardToColumnEnd(alpha, memberName);
      await expect.poll(() => driver.kanbanColumnCards(memberName), { timeout: 60_000 }).toContain(alpha);
      // Same optimistic-render race as the label step above — poll until
      // the cycle membership persists.
      await expect
        .poll(async () => (await serverIssueDetails(seed.workspaceSlug, projectId, alphaId, owner.cookie)).cycleId, {
          timeout: 60_000,
        })
        .toBe(cycleId);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-038"], "bug: moves into In Progress land in Backlog (NEWFRONT-160)"),
  { tag: specTags(["ISS-038"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB stuck ${suffix}`, `KS${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    const name = `KB stuck ${suffix}`;
    const id = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, name, home.id);
    await openBoard(driver, seed, projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(states.length);

    await test.step("the card renders moved but the server kept Backlog", async () => {
      // Intended: the issue lands in In Progress (UI and server agree).
      // Pinned: the drop applies optimistically in the UI, but moves into
      // In Progress, In Review, or In Test silently land in the default
      // state on the server (NEWFRONT-160).
      await driver.kanbanDragCardToColumnEnd(name, "In Progress");
      await expect.poll(() => driver.kanbanColumnCards("In Progress"), { timeout: 60_000 }).toContain(name);
      // The server read right after the drop is racy: it can briefly show
      // the move before settling on the default state (the post-rebase
      // re-verify read In Progress at assert time, Backlog minutes later).
      // Poll until two consecutive reads agree, then pin the settled value.
      let previous = "";
      let settled = "";
      for (let attempt = 0; attempt < 24; attempt += 1) {
        const details = await serverIssueDetails(seed.workspaceSlug, projectId, id, owner.cookie);
        settled = details.stateId;
        if (settled === previous) break;
        previous = settled;
        await new Promise((resolve) => setTimeout(resolve, 5000));
      }
      expect(settled).toBe(home.id);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-039"], "move a card across swimlanes"),
  { tag: specTags(["ISS-039"]) },
  async ({ driver, seed }) => {
    // A scratch project keeps the lanes exact: the seed's unlabeled rows
    // never reach a labels sub-grouped board (NEWFRONT-158); same
    // workaround as ISS-029.
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB move ${suffix}`, `KM${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    // The destination is the nearest default state, not a fresh trailing
    // one: a created state lands columns off-screen, outside the
    // virtualized content window, where the drop target never renders.
    const dest =
      states.find((state) => !state.isDefault && state.group === home.group) ??
      states.find((state) => !state.isDefault);
    if (!dest) throw new Error("[parity] scratch project has no second state.");
    const doneId = dest.id;
    const label = await serverCreateLabel(seed.workspaceSlug, projectId, `KB move ${suffix}`, "#666666", owner.cookie);
    const alpha = `KB move a ${suffix}`;
    const beta = `KB move b ${suffix}`;
    const gamma = `KB move c ${suffix}`;
    const alphaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, alpha, home.id);
    const betaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, beta, home.id);
    const gammaId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, gamma, home.id);
    await serverPatchIssue(
      seed.workspaceSlug,
      projectId,
      gammaId,
      { state_id: doneId, label_ids: [label.id] },
      owner.cookie
    );
    const doneName = dest.name;
    const ctx = await openBoard(driver, seed, projectId, { group_by: "state", sub_group_by: "labels" });
    await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).toHaveLength(2);

    await test.step("dropping across lanes updates both values", async () => {
      await driver.kanbanDragCardBefore(alpha, gamma);
      await expect.poll(() => driver.kanbanCellCards(doneName, label.name), { timeout: 60_000 }).toContain(alpha);
      const details = await serverIssueDetails(seed.workspaceSlug, projectId, alphaId, owner.cookie);
      expect(details.stateId).toBe(doneId);
      expect(details.labelIds).toContain(label.id);
    });

    await test.step("a non-draggable dimension refuses the move", async () => {
      // State groups render swimlanes but are not a draggable dimension
      // (target_date lanes crash the board: NEWFRONT-159). Gamma moves to
      // Done so the three issues span three groups; beta stays home.
      // Fixtures must avoid In Progress/In Review/In Test — moves there
      // silently land in Backlog (NEWFRONT-160).
      const completedState = states.find((state) => state.group === "completed");
      if (!completedState) throw new Error("[parity] scratch project lacks a completed state.");
      await serverPatchIssue(seed.workspaceSlug, projectId, gammaId, { state_id: completedState.id }, owner.cookie);
      await setBoardFilters(driver, seed, projectId, ctx, { sub_group_by: "state_detail.group" });
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).not.toHaveLength(0);
      await expect
        .poll(async () => (await driver.kanbanCards()).map((card) => card.name), { timeout: 120_000 })
        .toEqual(expect.arrayContaining([alpha, beta, gamma]));
      const placed = await driver.kanbanCards();
      const betaBefore = placed.find((card) => card.name === beta);
      // A refused move never settles, so attempt without settling and assert
      // the card never left its cell.
      await driver.kanbanAttemptCardBefore(beta, gamma);
      expect((await driver.kanbanCards()).find((card) => card.name === beta)).toEqual(betaBefore);
      const details = await serverIssueDetails(seed.workspaceSlug, projectId, betaId, owner.cookie);
      expect(details.stateId).toBe(home.id);
      expect(details.labelIds).toHaveLength(0);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-039"], "bug: target_date swimlanes render a blank board (NEWFRONT-159)"),
  { tag: specTags(["ISS-039"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB dates ${suffix}`, `KD${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    const aId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB dates a ${suffix}`, home.id);
    const bId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB dates b ${suffix}`, home.id);
    const cId = await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB dates c ${suffix}`, home.id);
    await serverPatchIssue(seed.workspaceSlug, projectId, aId, { target_date: "2026-12-01" }, owner.cookie);
    await serverPatchIssue(seed.workspaceSlug, projectId, bId, { target_date: "2026-12-02" }, owner.cookie);
    await serverPatchIssue(seed.workspaceSlug, projectId, cId, { target_date: "2026-12-03" }, owner.cookie);
    const ctx = await openBoard(driver, seed, projectId, { group_by: "state" });

    await test.step("the dated lanes never mount", async () => {
      // Intended: one swimlane per target date with its issue as a card.
      // Pinned: the sub-grouped fetch succeeds but the board body stays
      // blank — no lanes ever mount (NEWFRONT-159). Bounded: the flat
      // board above warms the host, and a healthy board mounts lanes in
      // seconds once kanban is active, so 60 empty seconds is the crash.
      const current = await serverProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie);
      await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, ctx.user.cookie, {
        display_filters: { ...current.displayFilters, sub_group_by: "target_date" },
      });
      await boardReloadWithRetry(driver);
      await expect.poll(() => driver.boardActiveLayout(), { timeout: 120_000 }).toBe("kanban");
      let mounted = false;
      try {
        await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 60_000 }).not.toHaveLength(0);
        mounted = true;
      } catch {
        mounted = false;
      }
      expect(mounted).toBe(false);
      const rows = await serverIssues(seed.workspaceSlug, projectId, owner.cookie);
      expect(rows).toHaveLength(3);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-040"], "drop a card on the delete zone"),
  { tag: specTags(["ISS-040"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const title = `KB doomed ${uniqueSuffix().slice(0, 6)}`;
    const states = await serverListStates(seed.workspaceSlug, seed.projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] seed project has no states.");
    const id = await serverCreateIssue(seed.workspaceSlug, seed.projectId, owner.cookie, title, home.id);
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanCards(), { timeout: 120_000 }).toHaveLength(4);

    await test.step("dropping on the zone asks for confirmation", async () => {
      await driver.kanbanDragCardToDelete(title);
      expect(await driver.kanbanDeleteModalVisible()).toBe(true);
    });

    await test.step("confirming deletes the issue", async () => {
      await driver.kanbanConfirmDelete();
      await expect.poll(() => driver.kanbanCards(), { timeout: 60_000 }).toHaveLength(3);
      await expect(serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).rejects.toThrow();
      const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
      expect(rows).toHaveLength(3);
    });

    await test.step("cleanup restores the seed preferences", async () => {
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-041"], "completed cycles refuse kanban drops"),
  { tag: specTags(["ISS-041"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6);
    const cycleName = `KB past ${suffix}`;
    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      cycleName,
      "2026-01-05",
      "2026-01-12",
      owner.cookie
    );
    const name = seed.issueNames[0] ?? "";
    const ctx = await openBoard(driver, seed, seed.projectId, { group_by: "cycle" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(2);
    const none = (await driver.kanbanColumns()).find((entry) => entry.name !== cycleName)?.name ?? "";

    await test.step("the completed column overlays and keeps the card", async () => {
      const { overlay } = await driver.kanbanDragHoldOverColumn(name, cycleName);
      expect(overlay).not.toBeNull();
      expect(overlay ?? "").toMatch(/complet/i);
      expect(await driver.kanbanColumnCards(none)).toContain(name);
      const id = await issueIdByName(seed, seed.projectId, owner.cookie, name);
      expect((await serverIssueDetails(seed.workspaceSlug, seed.projectId, id, owner.cookie)).cycleId).toBeNull();
    });

    await test.step("cleanup removes the cycle and restores preferences", async () => {
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, owner.cookie);
      await restoreBoard(seed, seed.projectId, ctx);
    });
  }
);

test(
  specTitle(["ISS-042"], "kanban per-column pagination"),
  { tag: specTags(["ISS-042"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB pages ${suffix}`, `KP${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    // Created sequentially so the server order matches the name order and
    // the last name is deterministically on the last page.
    const names: string[] = [];
    for (let index = 0; index < 32; index += 1) {
      const name = `KB page ${suffix} ${String(index + 1).padStart(2, "0")}`;
      names.push(name);
      await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, name, home.id);
    }
    const last = names[names.length - 1] ?? "";
    const ctx = await openBoard(driver, seed, projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumnCards(home.name), { timeout: 120_000 }).not.toHaveLength(0);

    await test.step("flat columns auto-load the next page on scroll", async () => {
      expect(await driver.kanbanColumnCards(home.name)).not.toContain(last);
      for (let attempt = 0; attempt < 4; attempt += 1) {
        await driver.kanbanColumnScrollEnd(home.name);
        const cards = await driver.kanbanColumnCards(home.name);
        if (cards.includes(last)) break;
        if (attempt === 3) throw new Error("[parity] last page never auto-loaded.");
      }
      expect(await serverIssues(seed.workspaceSlug, projectId, owner.cookie)).toHaveLength(32);
    });

    await test.step("sub-grouped cells page behind a load-more entry", async () => {
      await setBoardFilters(driver, seed, projectId, ctx, { sub_group_by: "state_detail.group" });
      await expect.poll(() => driver.kanbanSwimlanes(), { timeout: 120_000 }).not.toHaveLength(0);
      const lanes = await driver.kanbanSwimlanes();
      const full = lanes.find((entry) => entry.count > 10)?.name ?? "";
      expect(full).not.toBe("");
      expect(await driver.kanbanCellHasLoadMore(home.name, full)).toBe(true);
      await driver.kanbanCellLoadMore(home.name, full);
      expect((await driver.kanbanCellCards(home.name, full)).length).toBeGreaterThan(10);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);

test(
  specTitle(["ISS-043"], "kanban virtualization window and drag auto-scroll"),
  { tag: specTags(["ISS-043"]) },
  async ({ driver, seed }) => {
    const owner = await signInFreshUser(seed.email, seed.password);
    const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
    const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB virt ${suffix}`, `KV${suffix}`);
    const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
    const home = states.find((state) => state.isDefault) ?? states[0];
    if (!home) throw new Error("[parity] scratch project has no states.");
    for (let extra = states.length; extra < 8; extra += 1) {
      await serverCreateState(seed.workspaceSlug, projectId, `KB pad ${suffix} ${extra}`, "unstarted", owner.cookie);
    }
    // Created sequentially so the server order matches the name order and
    // the last name is deterministically at the column end.
    const names: string[] = [];
    for (let index = 0; index < 61; index += 1) {
      const name = `KB virt ${suffix} ${String(index + 1).padStart(2, "0")}`;
      names.push(name);
      await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, name, home.id);
    }
    const last = names[names.length - 1] ?? "";
    await openBoard(driver, seed, projectId, { group_by: "state" });
    await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(8);

    await test.step("only a window of cards renders content at once", async () => {
      for (let attempt = 0; attempt < 6; attempt += 1) {
        await driver.kanbanColumnScrollEnd(home.name);
        if ((await driver.kanbanColumnCards(home.name)).includes(last)) break;
        if (attempt === 5) throw new Error("[parity] tall column never reached its end.");
      }
      const rendered = await driver.kanbanColumnCards(home.name);
      expect(rendered).toContain(last);
      // Loaded shells mount for every issue, but only the visible window
      // renders card content; the rest stay height-estimated placeholders.
      const contentful = rendered.filter((name) => name !== "");
      expect(contentful.length).toBeGreaterThan(0);
      expect(contentful.length).toBeLessThan(61);
      expect(await serverIssues(seed.workspaceSlug, projectId, owner.cookie)).toHaveLength(61);
    });

    await test.step("dragging near the right edge auto-scrolls the board", async () => {
      // Hold from the middle of the rendered window: edge cards churn as
      // the virtualized window settles. The board rests at x = 0 with
      // headroom to the right.
      const contentful = (await driver.kanbanColumnCards(home.name)).filter((name) => name !== "");
      const held = contentful[Math.floor(contentful.length / 2)] ?? "";
      expect(held).not.toBe("");
      const before = await driver.kanbanBoardScroll();
      await driver.kanbanDragHoldNearEdge(held, "right", 2500);
      const after = await driver.kanbanBoardScroll();
      expect(after.x).toBeGreaterThan(before.x);
    });

    await test.step("dragging near the bottom edge auto-scrolls the column", async () => {
      // A reload resets the column to its top so the downward hold has room.
      await kanbanReloadWithRetry(driver);
      await expect.poll(() => driver.kanbanColumns(), { timeout: 120_000 }).toHaveLength(8);
      const contentful = (await driver.kanbanColumnCards(home.name)).filter((name) => name !== "");
      const held = contentful[Math.floor(contentful.length / 2)] ?? "";
      expect(held).not.toBe("");
      const before = await driver.kanbanColumnScroll(home.name);
      await driver.kanbanDragHoldNearEdge(held, "bottom", 2500);
      const after = await driver.kanbanColumnScroll(home.name);
      expect(after.y).toBeGreaterThan(before.y);
    });

    await test.step("cleanup removes the scratch project", async () => {
      await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
    });
  }
);
