// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-449): public-board layouts — the layout
// switcher (SPC-020), shareable view addresses (SPC-021), view validation
// (SPC-022), list grouping (SPC-023), board columns (SPC-024), the empty
// board (SPC-025), and per-group paging (SPC-026). Green on apps/space.
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  createIssue,
  createState,
  deleteState,
  parityProjectIdentifier,
  serverCreateProject,
  serverDeleteProject,
  serverPatchBoard,
  serverPublicBoardSettings,
  serverPublicIssues,
  serverPublicStates,
  serverPublishBoard,
  serverPublishBoardFacts,
  serverUnpublishBoard,
  signInSession,
  uniqueSuffixForProjects,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_020 = ["SPC-020"];
const ROW_021 = ["SPC-021"];
const ROW_022 = ["SPC-022"];
const ROW_023 = ["SPC-023"];
const ROW_024 = ["SPC-024"];
const ROW_025 = ["SPC-025"];
const ROW_026 = ["SPC-026"];

/** Seed shape the specs converge every flipped board back to. */
const SEEDED_VIEWS = { list: true, kanban: true, calendar: true, gantt: true, spreadsheet: true };
const SEEDED_FLAGS = { is_votes_enabled: false, is_reactions_enabled: false, is_comments_enabled: false };

/** Board anchor or a loud failure when the stack predates the seed hunk. */
function requireAnchor(seed: ParitySeedFacts): string {
  if (seed.spaceAnchor === undefined || seed.spaceAnchor === "") {
    throw new Error("[parity] seed carries no spaceAnchor; reseed the stack (NEWFRONT-97 hunk).");
  }
  return seed.spaceAnchor;
}

/** Converge the seeded board to the seed shape (views + flags), verified. */
async function convergeSeededBoard(
  seed: ParitySeedFacts,
  cookie: string,
  boardId: string,
  anchor: string
): Promise<void> {
  await serverPatchBoard(
    seed.workspaceSlug,
    seed.projectId,
    boardId,
    { ...SEEDED_FLAGS, view_props: SEEDED_VIEWS },
    cookie
  );
  const settings = await serverPublicBoardSettings(anchor, cookie);
  expect(settings.viewProps).toEqual(SEEDED_VIEWS);
  expect(settings.isVotesEnabled).toBe(false);
  expect(settings.isReactionsEnabled).toBe(false);
  expect(settings.isCommentsEnabled).toBe(false);
}

test(
  specTitle(ROW_020, "switcher offers exactly the publisher-enabled layouts"),
  { tag: specTags(ROW_020) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const facts = await serverPublishBoardFacts(seed.workspaceSlug, seed.projectId, cookie);
    if (facts === null) throw new Error("[parity] seeded project has no published board.");
    await convergeSeededBoard(seed, cookie, facts.id, anchor);
    try {
      await test.step("all views enabled offers the two board layouts", async () => {
        const settings = await serverPublicBoardSettings(anchor, cookie);
        expect(settings.viewProps).toEqual(SEEDED_VIEWS);
        await driver.spaceOpenBoard(anchor);
        expect(await driver.spaceBoardLayoutOptions()).toEqual(["list", "kanban"]);
      });

      await test.step("disabling a view removes it from the switcher", async () => {
        await serverPatchBoard(
          seed.workspaceSlug,
          seed.projectId,
          facts.id,
          { view_props: { ...SEEDED_VIEWS, kanban: false } },
          cookie
        );
        expect((await serverPublicBoardSettings(anchor, cookie)).viewProps["kanban"]).toBe(false);
        await driver.spaceOpenBoard(anchor);
        expect(await driver.spaceBoardLayoutOptions()).toEqual(["list"]);
      });

      await test.step("single-view boards offer no choice the other way either", async () => {
        await serverPatchBoard(
          seed.workspaceSlug,
          seed.projectId,
          facts.id,
          { view_props: { ...SEEDED_VIEWS, list: false } },
          cookie
        );
        expect((await serverPublicBoardSettings(anchor, cookie)).viewProps["list"]).toBe(false);
        await driver.spaceOpenBoard(anchor);
        expect(await driver.spaceBoardLayoutOptions()).toEqual(["kanban"]);
      });
    } finally {
      // Best-effort: a failing restore must not mask the scenario's own
      // failure. The next flip scenario converges (asserted) at its start,
      // so a broken restore still surfaces loudly.
      await convergeSeededBoard(seed, cookie, facts.id, anchor).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_021, "layout switch rewrites the address and flips without refetching"),
  { tag: specTags(ROW_021) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const facts = await serverPublishBoardFacts(seed.workspaceSlug, seed.projectId, cookie);
    if (facts === null) throw new Error("[parity] seeded project has no published board.");
    await convergeSeededBoard(seed, cookie, facts.id, anchor);
    try {
      await test.step("switching flips the layout and carries the view in the address", async () => {
        await driver.spaceOpenBoard(anchor);
        expect(await driver.spaceBoardActiveLayout()).toBe("list");
        await driver.spaceBoardSwitchLayout("kanban");
        expect(await driver.spaceBoardActiveLayout()).toBe("kanban");
        expect(driver.page.url()).toContain("board=kanban");
        expect((await driver.spaceBoardKanbanColumns()).length).toBeGreaterThan(0);
        await driver.spaceBoardSwitchLayout("list");
        expect(await driver.spaceBoardActiveLayout()).toBe("list");
        expect(driver.page.url()).toContain("board=list");
        expect((await driver.spaceBoardListGroups()).length).toBeGreaterThan(0);
      });

      await test.step("a pasted address opens that view directly", async () => {
        await driver.spaceOpenBoard(anchor, "?board=kanban");
        expect(await driver.spaceBoardActiveLayout()).toBe("kanban");
        expect((await driver.spaceBoardKanbanColumns()).length).toBeGreaterThan(0);
        await driver.spaceOpenBoard(anchor, "?board=list");
        expect(await driver.spaceBoardActiveLayout()).toBe("list");
        expect((await driver.spaceBoardListGroups()).length).toBeGreaterThan(0);
      });

      await test.step("flips reuse the loaded issues without refetching", async () => {
        await driver.spaceOpenBoard(anchor, "?board=list");
        expect(await driver.spaceBoardSwitchLayoutRefetches("kanban")).toBe(0);
        expect(await driver.spaceBoardSwitchLayoutRefetches("list")).toBe(0);
      });
    } finally {
      // Best-effort: a failing restore must not mask the scenario's own
      // failure. The next flip scenario converges (asserted) at its start,
      // so a broken restore still surfaces loudly.
      await convergeSeededBoard(seed, cookie, facts.id, anchor).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_022, "unknown or missing view params fall back with an address rewrite"),
  { tag: specTags(ROW_022) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const facts = await serverPublishBoardFacts(seed.workspaceSlug, seed.projectId, cookie);
    if (facts === null) throw new Error("[parity] seeded project has no published board.");
    await convergeSeededBoard(seed, cookie, facts.id, anchor);
    try {
      await test.step("missing view settles on the first enabled view", async () => {
        await driver.spaceOpenBoard(anchor);
        expect(await driver.spaceBoardActiveLayout()).toBe("list");
        expect(driver.page.url()).toContain("board=list");
      });

      await test.step("unrecognised view rewrites to the first enabled view", async () => {
        await driver.spaceOpenBoard(anchor, "?board=bogus");
        expect(await driver.spaceBoardActiveLayout()).toBe("list");
        expect(driver.page.url()).toContain("board=list");
        expect(driver.page.url()).not.toContain("board=bogus");
      });

      await test.step("fallback follows the enabled set when list is off", async () => {
        await serverPatchBoard(
          seed.workspaceSlug,
          seed.projectId,
          facts.id,
          { view_props: { ...SEEDED_VIEWS, list: false } },
          cookie
        );
        await driver.spaceOpenBoard(anchor, "?board=list");
        expect(await driver.spaceBoardActiveLayout()).toBe("kanban");
        expect(driver.page.url()).toContain("board=kanban");
      });
    } finally {
      // Best-effort: a failing restore must not mask the scenario's own
      // failure. The next flip scenario converges (asserted) at its start,
      // so a broken restore still surfaces loudly.
      await convergeSeededBoard(seed, cookie, facts.id, anchor).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_023, "list layout groups issues under state headers including empty states"),
  { tag: specTags(ROW_023) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const emptyName = `Probe Empty ${uniqueSuffixForProjects()}`;
    const state = await createState(seed.workspaceSlug, seed.projectId, cookie, emptyName);
    try {
      await driver.spaceOpenBoard(anchor, "?board=list");
      const groups = await driver.spaceBoardListGroups();
      const states = await serverPublicStates(anchor, cookie);
      await test.step("every workflow state renders a header", async () => {
        expect(groups.map((g) => g.name).sort()).toEqual(states.map((s) => s.name).sort());
      });
      await test.step("populated groups render their rows and empty states render bare headers", async () => {
        const server = await serverPublicIssues(anchor, {}, cookie);
        const populated = groups.find((g) => g.rows > 0);
        if (populated === undefined) throw new Error("[parity] list shows no populated group.");
        expect(populated.rows).toBe(Math.min(50, server.totalCount));
        expect(populated.rows).toBeGreaterThan(0);
        const empty = groups.find((g) => g.name === emptyName);
        expect(empty).toBeDefined();
        expect(empty?.count).toBe(0);
        expect(empty?.rows).toBe(0);
      });
    } finally {
      await deleteState(seed.workspaceSlug, seed.projectId, state.id, cookie).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_023, "bug: NEWFRONT-454 list header count follows the server group total"),
  { tag: specTags(ROW_023) },
  async ({ driver, seed }) => {
    // The header faithfully renders the server's per-group total — but that
    // total wrongly excludes the intake-declined issue the rows still show
    // (NEWFRONT-454). Intended: the header count always equals the rows
    // beneath it. This pins the current render until the server is fixed.
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    await driver.spaceOpenBoard(anchor, "?board=list");
    const groups = await driver.spaceBoardListGroups();
    const server = await serverPublicIssues(anchor, {}, cookie);
    const populated = groups.find((g) => g.rows > 0);
    if (populated === undefined) throw new Error("[parity] list shows no populated group.");
    const serverGroup = server.groups.find((g) => g.issues.length > 0);
    if (serverGroup === undefined) throw new Error("[parity] public issues carry no populated group.");
    expect(populated.count).toBe(serverGroup.total);
    expect(populated.rows).toBe(Math.min(50, server.totalCount));
    expect(server.totalCount).toBe(serverGroup.total + 1);
  }
);

test(
  specTitle(ROW_024, "board layout renders state columns sideways including empty columns"),
  { tag: specTags(ROW_024) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const emptyName = `Probe Column ${uniqueSuffixForProjects()}`;
    const state = await createState(seed.workspaceSlug, seed.projectId, cookie, emptyName);
    try {
      await driver.spaceOpenBoard(anchor, "?board=kanban");
      const columns = await driver.spaceBoardKanbanColumns();
      const states = await serverPublicStates(anchor, cookie);
      await test.step("every workflow state renders a sideways column", async () => {
        expect(columns.map((c) => c.name).sort()).toEqual(states.map((s) => s.name).sort());
        expect(await driver.spaceBoardKanbanColumnsFlow()).toBe("row");
      });
      await test.step("populated columns render their cards and empty states render bare columns", async () => {
        const server = await serverPublicIssues(anchor, {}, cookie);
        const populated = columns.find((c) => c.rows > 0);
        if (populated === undefined) throw new Error("[parity] board shows no populated column.");
        expect(populated.rows).toBe(Math.min(50, server.totalCount));
        expect(populated.rows).toBeGreaterThan(0);
        const empty = columns.find((c) => c.name === emptyName);
        expect(empty).toBeDefined();
        expect(empty?.count).toBe(0);
        expect(empty?.rows).toBe(0);
      });
    } finally {
      await deleteState(seed.workspaceSlug, seed.projectId, state.id, cookie).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_024, "bug: NEWFRONT-454 board column count follows the server group total"),
  { tag: specTags(ROW_024) },
  async ({ driver, seed }) => {
    // Same server miscount as the list header (NEWFRONT-454): the column
    // renders the short group total while the rows beneath include the
    // intake-declined issue. Intended: count equals cards.
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    await driver.spaceOpenBoard(anchor, "?board=kanban");
    const columns = await driver.spaceBoardKanbanColumns();
    const server = await serverPublicIssues(anchor, {}, cookie);
    const populated = columns.find((c) => c.rows > 0);
    if (populated === undefined) throw new Error("[parity] board shows no populated column.");
    const serverGroup = server.groups.find((g) => g.issues.length > 0);
    if (serverGroup === undefined) throw new Error("[parity] public issues carry no populated group.");
    expect(populated.count).toBe(serverGroup.total);
    expect(populated.rows).toBe(Math.min(50, server.totalCount));
    expect(server.totalCount).toBe(serverGroup.total + 1);
  }
);

/** Fresh published project with no issues; caller owns teardown. */
async function setupEmptyPublishedProject(
  seed: ParitySeedFacts,
  cookie: string,
  prefix: string
): Promise<{ projectId: string; boardId: string; anchor: string; owned: boolean }> {
  const tag = uniqueSuffixForProjects();
  const projectId = await serverCreateProject(
    seed.workspaceSlug,
    cookie,
    `${prefix} ${tag}`,
    parityProjectIdentifier(
      prefix
        .replace(/[^A-Za-z]/g, "")
        .slice(0, 4)
        .toUpperCase() || "PX"
    )
  );
  const board = await serverPublishBoard(seed.workspaceSlug, projectId, cookie);
  const published = await serverPublishBoardFacts(seed.workspaceSlug, projectId, cookie);
  if (published === null || published.anchor === null) {
    throw new Error("[parity] fresh project publish carried no anchor.");
  }
  return { projectId, boardId: board.boardId, anchor: published.anchor, owned: board.owned };
}

/** Tear down a project created by setupEmptyPublishedProject. Warns instead of
 * throwing so teardown never masks the scenario's own failure. */
async function teardownPublishedProject(
  seed: ParitySeedFacts,
  cookie: string,
  world: { projectId: string; boardId: string; owned: boolean }
): Promise<void> {
  if (world.owned) {
    await serverUnpublishBoard(seed.workspaceSlug, world.projectId, world.boardId, cookie).catch((error: unknown) => {
      // eslint-disable-next-line no-console -- teardown noise stays in the log.
      console.warn(`[parity] board unpublish for ${world.projectId} failed: ${String(error).slice(0, 160)}`);
    });
  }
  await serverDeleteProject(seed.workspaceSlug, world.projectId, cookie).catch((error: unknown) => {
    // eslint-disable-next-line no-console -- teardown noise stays in the log.
    console.warn(`[parity] project delete for ${world.projectId} failed: ${String(error).slice(0, 160)}`);
  });
}

test(
  specTitle(ROW_025, "board with zero published issues shows the dedicated empty state"),
  { tag: specTags(ROW_025) },
  async ({ driver, seed }) => {
    const cookie = await signInSession(seed.email, seed.password);
    const world = await setupEmptyPublishedProject(seed, cookie, "SPC025 Empty");
    try {
      const server = await serverPublicIssues(world.anchor, {}, cookie);
      expect(server.totalCount).toBe(0);
      await test.step("list layout shows the empty notice on a live board shell", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=list");
        expect(await driver.spaceBoardEmptyVisible()).toBe(true);
        expect(await driver.spaceBoardLayoutOptions()).toEqual(["list", "kanban"]);
      });
      await test.step("board layout shows the same empty notice", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=kanban");
        expect(await driver.spaceBoardEmptyVisible()).toBe(true);
      });
      await test.step("boards with issues render no empty notice", async () => {
        await driver.spaceOpenBoard(requireAnchor(seed), "?board=list");
        expect(await driver.spaceBoardEmptyVisible()).toBe(false);
      });
    } finally {
      await teardownPublishedProject(seed, cookie, world);
    }
  }
);

/** Provision `count` issues with a unique prefix; resolves with their ids. */
async function provisionIssues(
  seed: ParitySeedFacts,
  cookie: string,
  projectId: string,
  prefix: string,
  count: number
): Promise<string[]> {
  const tag = uniqueSuffixForProjects();
  const ids: string[] = [];
  for (let i = 0; i < count; i++) {
    const created = await createIssue(
      seed.workspaceSlug,
      projectId,
      cookie,
      `${prefix} ${tag} ${String(i).padStart(3, "0")}`
    );
    ids.push(created.id);
  }
  return ids;
}

test(
  specTitle(ROW_026, "list groups page independently through the manual control with placeholder rows"),
  { tag: specTags(ROW_026) },
  async ({ driver, seed }) => {
    const cookie = await signInSession(seed.email, seed.password);
    const world = await setupEmptyPublishedProject(seed, cookie, "SPC026 List");
    try {
      await provisionIssues(seed, cookie, world.projectId, "SPC026 list", 55);
      const server = await serverPublicIssues(world.anchor, {}, cookie);
      expect(server.totalCount).toBe(55);
      const states = await serverPublicStates(world.anchor, cookie);
      expect(states.length).toBeGreaterThan(1);

      await test.step("the first page renders fifty rows with a manual control", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=list");
        const groups = await driver.spaceBoardListGroups();
        const populated = groups.find((g) => g.rows > 0);
        if (populated === undefined) throw new Error("[parity] list shows no populated group.");
        expect(populated.rows).toBe(50);
        expect(populated.count).toBe(55);
        expect(await driver.spaceBoardListGroupLoadMoreVisible(populated.name)).toBe(true);
        for (const group of groups) {
          if (group.name !== populated.name) expect(group.rows).toBe(0);
        }
      });

      await test.step("the manual control loads the next page and retires at the end", async () => {
        const populated = (await driver.spaceBoardListGroups()).find((g) => g.rows > 0);
        if (populated === undefined) throw new Error("[parity] list shows no populated group.");
        await driver.spaceBoardListGroupLoadMore(populated.name);
        const after = (await driver.spaceBoardListGroups()).find((g) => g.name === populated.name);
        expect(after?.rows).toBe(55);
        expect(await driver.spaceBoardListGroupLoadMoreVisible(populated.name)).toBe(false);
      });

      await test.step("paging shows placeholder rows and leaves other groups untouched", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=list");
        const populated = (await driver.spaceBoardListGroups()).find((g) => g.rows > 0);
        if (populated === undefined) throw new Error("[parity] list shows no populated group.");
        expect(await driver.spaceBoardListGroupShowsLoader(populated.name)).toBe(true);
        const after = await driver.spaceBoardListGroups();
        expect(after.find((g) => g.name === populated.name)?.rows).toBe(55);
        for (const group of after) {
          if (group.name !== populated.name) expect(group.rows).toBe(0);
        }
      });
    } finally {
      await teardownPublishedProject(seed, cookie, world);
    }
  }
);

test(
  specTitle(ROW_026, "board columns page on scroll with placeholders and a debounced loader"),
  { tag: specTags(ROW_026) },
  async ({ driver, seed }) => {
    const cookie = await signInSession(seed.email, seed.password);
    const world = await setupEmptyPublishedProject(seed, cookie, "SPC026 Board");
    try {
      await provisionIssues(seed, cookie, world.projectId, "SPC026 board", 55);
      const server = await serverPublicIssues(world.anchor, {}, cookie);
      expect(server.totalCount).toBe(55);

      await test.step("scroll bursts coalesce into a single column fetch", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=kanban");
        const populated = (await driver.spaceBoardKanbanColumns()).find((c) => c.rows > 0);
        if (populated === undefined) throw new Error("[parity] board shows no populated column.");
        expect(populated.rows).toBe(50);
        expect(await driver.spaceBoardKanbanScrollBurstFetches(populated.name, 5)).toBe(1);
        const after = (await driver.spaceBoardKanbanColumns()).find((c) => c.name === populated.name);
        expect(after?.rows).toBe(55);
      });

      await test.step("scrolling to the end pages with placeholder cards, other columns untouched", async () => {
        await driver.spaceOpenBoard(world.anchor, "?board=kanban");
        const populated = (await driver.spaceBoardKanbanColumns()).find((c) => c.rows > 0);
        if (populated === undefined) throw new Error("[parity] board shows no populated column.");
        expect(await driver.spaceBoardKanbanColumnShowsLoader(populated.name)).toBe(true);
        const after = await driver.spaceBoardKanbanColumns();
        expect(after.find((c) => c.name === populated.name)?.rows).toBe(55);
        for (const column of after) {
          if (column.name !== populated.name) expect(column.rows).toBe(0);
        }
      });
    } finally {
      await teardownPublishedProject(seed, cookie, world);
    }
  }
);
