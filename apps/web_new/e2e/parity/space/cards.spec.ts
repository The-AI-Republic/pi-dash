// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-449): public-board cards — public key and
// title (SPC-027), property markers (SPC-028), vote/reaction footers
// (SPC-029), opening cards (SPC-030), peek highlight (SPC-031), read-only
// board (SPC-032), and no live refresh (SPC-033). Green on apps/space.
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  createCycle,
  createIssue,
  createModule,
  deleteCycle,
  deleteIssue,
  deleteModule,
  serverAddIssuesToCycle,
  serverAddIssuesToModule,
  serverCreateLabel,
  serverDeleteLabel,
  serverPatchBoard,
  serverPublicBoardSettings,
  serverPublicIssues,
  serverPublishBoardFacts,
  signInSession,
  uniqueSuffixForProjects,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_027 = ["SPC-027"];
const ROW_028 = ["SPC-028"];
const ROW_029 = ["SPC-029"];
const ROW_030 = ["SPC-030"];
const ROW_031 = ["SPC-031"];
const ROW_032 = ["SPC-032"];
const ROW_033 = ["SPC-033"];

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

/** Best-effort issue cleanup so teardown never masks an assertion. */
async function deleteIssuesQuietly(
  seed: ParitySeedFacts,
  cookie: string,
  projectId: string,
  issueIds: string[]
): Promise<void> {
  for (const id of issueIds) {
    try {
      await deleteIssue(seed.workspaceSlug, projectId, id, cookie);
    } catch (error) {
      // eslint-disable-next-line no-console -- teardown noise stays in the log.
      console.warn(`[parity] issue cleanup for ${id} failed: ${String(error).slice(0, 160)}`);
    }
  }
}

/** Sequence number out of a rendered public key (`PREFIX-123`). */
function sequenceOfKey(key: string): number {
  const match = /^([A-Za-z]+)-(\d+)$/.exec(key);
  if (match === null) throw new Error(`[parity] public key ${JSON.stringify(key)} has no code-plus-number shape.`);
  return Number(match[2]);
}

/** Prefix out of a rendered public key. */
function prefixOfKey(key: string): string {
  const match = /^([A-Za-z]+)-(\d+)$/.exec(key);
  if (match === null) throw new Error(`[parity] public key ${JSON.stringify(key)} has no code-plus-number shape.`);
  const prefix = match[1];
  if (prefix === undefined) throw new Error("[parity] public key matched an empty prefix.");
  return prefix;
}

test(
  specTitle(ROW_027, "cards show the public key and title with the full title on hover"),
  { tag: specTags(ROW_027) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const longTitle = `SPC027 long title ${uniqueSuffixForProjects()} ${"lorem ipsum dolor sit amet ".repeat(5).trim()}`;
    const created = await createIssue(seed.workspaceSlug, seed.projectId, cookie, longTitle);
    try {
      const server = await serverPublicIssues(anchor, {}, cookie);
      const byName = new Map(server.groups.flatMap((g) => g.issues).map((i) => [i.name, i]));
      await test.step("list cards pair the public key with the title", async () => {
        await driver.spaceOpenBoard(anchor, "?board=list");
        const prefixes = new Set<string>();
        for (const name of [...seed.issueNames, longTitle]) {
          const facts = await driver.spaceBoardCardFacts(name);
          expect(facts.title).toBe(name);
          expect(facts.hoverTitle).toBe(name);
          prefixes.add(prefixOfKey(facts.key));
          expect(sequenceOfKey(facts.key)).toBe(byName.get(name)?.sequenceId);
        }
        expect(prefixes.size).toBe(1);
      });
      await test.step("board cards pair the same key with the title", async () => {
        await driver.spaceOpenBoard(anchor, "?board=kanban");
        const facts = await driver.spaceBoardCardFacts(longTitle);
        expect(facts.title).toBe(longTitle);
        expect(facts.hoverTitle).toBe(longTitle);
        expect(sequenceOfKey(facts.key)).toBe(byName.get(longTitle)?.sequenceId);
      });
    } finally {
      await deleteIssuesQuietly(seed, cookie, seed.projectId, [created.id]);
    }
  }
);

test(
  specTitle(ROW_028, "cards carry state, priority, label and date markers"),
  { tag: specTags(ROW_028) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const tag = uniqueSuffixForProjects();
    const label = await serverCreateLabel(seed.workspaceSlug, seed.projectId, `SPC028 Label ${tag}`, "#3366CC", cookie);
    const richName = `SPC028 rich ${tag}`;
    const bareName = `SPC028 bare ${tag}`;
    const rich = await createIssue(seed.workspaceSlug, seed.projectId, cookie, richName, {
      priority: "high",
      target_date: "2027-03-15",
      label_ids: [label.id],
    });
    const bare = await createIssue(seed.workspaceSlug, seed.projectId, cookie, bareName);
    const cycle = await createCycle(seed.workspaceSlug, seed.projectId, cookie, `SPC028 Cycle ${tag}`);
    const module = await createModule(seed.workspaceSlug, seed.projectId, cookie, `SPC028 Module ${tag}`);
    await serverAddIssuesToCycle(seed.workspaceSlug, seed.projectId, cycle.id, [rich.id], cookie);
    await serverAddIssuesToModule(seed.workspaceSlug, seed.projectId, module.id, [rich.id], cookie);
    try {
      await test.step("rich cards mark state, priority, labels and the due date", async () => {
        await driver.spaceOpenBoard(anchor, "?board=list");
        const groups = await driver.spaceBoardListGroups();
        const populated = groups.find((g) => g.rows > 0);
        if (populated === undefined) throw new Error("[parity] list shows no populated group.");
        const markers = await driver.spaceBoardCardMarkers(richName);
        expect(markers.state).toBe(populated.name);
        expect(markers.priority).toBe("high");
        expect(markers.labels).toEqual([label.name]);
        const expectedDate = new Date(2027, 2, 15).toLocaleDateString("en-US", {
          month: "short",
          day: "numeric",
          year: "numeric",
        });
        expect(markers.dates).toContain(expectedDate);
      });
      await test.step("bare cards keep the state marker and drop the rest", async () => {
        const markers = await driver.spaceBoardCardMarkers(bareName);
        expect(markers.state).not.toBeNull();
        expect(markers.priority).toBe("none");
        expect(markers.labels).toEqual([]);
        expect(markers.dates).toEqual([]);
      });
      await test.step("cycle and module memberships leave no visible marker", async () => {
        const markers = await driver.spaceBoardCardMarkers(richName);
        expect(markers.text).not.toContain(cycle.name);
        expect(markers.text).not.toContain(module.name);
      });
    } finally {
      await deleteIssuesQuietly(seed, cookie, seed.projectId, [rich.id, bare.id]);
      await deleteCycle(seed.workspaceSlug, seed.projectId, cycle.id, cookie).catch(() => {});
      await deleteModule(seed.workspaceSlug, seed.projectId, module.id, cookie).catch(() => {});
      await serverDeleteLabel(seed.workspaceSlug, seed.projectId, label.id, cookie).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_029, "board card footers follow the publisher flags; list cards never show one"),
  { tag: specTags(ROW_029) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const facts = await serverPublishBoardFacts(seed.workspaceSlug, seed.projectId, cookie);
    if (facts === null) throw new Error("[parity] seeded project has no published board.");
    await convergeSeededBoard(seed, cookie, facts.id, anchor);
    const name = seed.issueNames[0];
    if (name === undefined) throw new Error("[parity] seed carries no issue names.");
    const matrix = [
      { votes: false, reactions: false },
      { votes: true, reactions: false },
      { votes: false, reactions: true },
      { votes: true, reactions: true },
    ];
    try {
      for (const flags of matrix) {
        await test.step(`votes ${String(flags.votes)} / reactions ${String(flags.reactions)}`, async () => {
          await serverPatchBoard(
            seed.workspaceSlug,
            seed.projectId,
            facts.id,
            { is_votes_enabled: flags.votes, is_reactions_enabled: flags.reactions },
            cookie
          );
          const settings = await serverPublicBoardSettings(anchor, cookie);
          expect(settings.isVotesEnabled).toBe(flags.votes);
          expect(settings.isReactionsEnabled).toBe(flags.reactions);
          await driver.spaceOpenBoard(anchor, "?board=kanban");
          expect(await driver.spaceBoardKanbanCardFooter(name)).toEqual({
            visible: flags.votes || flags.reactions,
            votes: flags.votes,
            reactions: flags.reactions,
          });
          await driver.spaceOpenBoard(anchor, "?board=list");
          expect(await driver.spaceBoardListCardFooterVisible(name)).toBe(false);
        });
      }
    } finally {
      // Best-effort: a failing restore must not mask the scenario's own
      // failure. The next flip scenario converges (asserted) at its start,
      // so a broken restore still surfaces loudly.
      await convergeSeededBoard(seed, cookie, facts.id, anchor).catch(() => {});
    }
  }
);

test(
  specTitle(ROW_030, "opening a card raises the overlay, records it, and highlights the list row"),
  { tag: specTags(ROW_030) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const server = await serverPublicIssues(anchor, {}, cookie);
    const byName = new Map(server.groups.flatMap((g) => g.issues).map((i) => [i.name, i]));
    const name = seed.issueNames[0];
    if (name === undefined) throw new Error("[parity] seed carries no issue names.");
    const id = byName.get(name)?.id;
    if (id === undefined) throw new Error(`[parity] public issues carry no ${JSON.stringify(name)}.`);
    await test.step("list click opens the overlay with the issue in the address", async () => {
      await driver.spaceOpenBoard(anchor, "?board=list");
      await driver.spaceBoardOpenCard(name);
      expect(await driver.spaceBoardPeekVisible()).toBe(true);
      expect(driver.page.url()).toContain(`peekId=${id}`);
      expect(driver.page.url()).toContain("board=list");
      expect(await driver.spaceBoardHighlightedRow()).toBe(name);
    });
    await test.step("board click opens the overlay with the view preserved", async () => {
      await driver.spaceOpenBoard(anchor, "?board=kanban");
      await driver.spaceBoardOpenCard(name);
      expect(await driver.spaceBoardPeekVisible()).toBe(true);
      expect(driver.page.url()).toContain(`peekId=${id}`);
      expect(driver.page.url()).toContain("board=kanban");
    });
  }
);

test(
  specTitle(ROW_031, "list highlight tracks the open issue and clears on close"),
  { tag: specTags(ROW_031) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const first = seed.issueNames[0];
    const second = seed.issueNames[1];
    if (first === undefined || second === undefined)
      throw new Error("[parity] seed carries fewer than two issue names.");
    const server = await serverPublicIssues(anchor, {}, cookie);
    const secondId = server.groups.flatMap((g) => g.issues).find((i) => i.name === second)?.id;
    if (secondId === undefined) throw new Error(`[parity] public issues carry no ${JSON.stringify(second)}.`);
    await driver.spaceOpenBoard(anchor, "?board=list");
    expect(await driver.spaceBoardHighlightedRow()).toBeNull();
    await driver.spaceBoardOpenCard(first);
    expect(await driver.spaceBoardHighlightedRow()).toBe(first);
    // The open overlay covers the second row's click point, so the second
    // issue opens through its address — the highlight must follow anyway.
    await driver.spaceOpenBoard(anchor, `?board=list&peekId=${secondId}`);
    expect(await driver.spaceBoardPeekVisible()).toBe(true);
    expect(await driver.spaceBoardHighlightedRow()).toBe(second);
    await driver.spaceBoardClosePeek();
    expect(await driver.spaceBoardPeekVisible()).toBe(false);
    expect(await driver.spaceBoardHighlightedRow()).toBeNull();
    expect(driver.page.url()).not.toContain("peekId=");
  }
);

test(
  specTitle(ROW_032, "board cards cannot be dragged, edited, or reordered"),
  { tag: specTags(ROW_032) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const name = seed.issueNames[0];
    if (name === undefined) throw new Error("[parity] seed carries no issue names.");
    await test.step("list cards expose no drag or edit surface and a drag attempt reorders nothing", async () => {
      await driver.spaceOpenBoard(anchor, "?board=list");
      expect(await driver.spaceBoardCardDragFacts()).toEqual({ draggable: 0, handles: 0, editControls: 0 });
      const order = await driver.spaceBoardDragCardOrder(name);
      expect(order.before.length).toBeGreaterThan(0);
      expect(order.after).toEqual(order.before);
    });
    await test.step("board cards expose no drag or edit surface either", async () => {
      await driver.spaceOpenBoard(anchor, "?board=kanban");
      expect(await driver.spaceBoardCardDragFacts()).toEqual({ draggable: 0, handles: 0, editControls: 0 });
    });
  }
);

test(
  specTitle(ROW_033, "refocusing the tab never refetches or reorders the board"),
  { tag: specTags(ROW_033) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const freshName = `SPC033 fresh ${uniqueSuffixForProjects()}`;
    await driver.spaceOpenBoard(anchor, "?board=list");
    const before = await driver.spaceBoardVisibleOrder();
    expect(before.length).toBeGreaterThan(0);
    const created = await createIssue(seed.workspaceSlug, seed.projectId, cookie, freshName);
    try {
      const stability = await driver.spaceBoardRefocusStability();
      expect(stability.before).toEqual(before);
      expect(stability.after).toEqual(before);
      expect(stability.refetches).toBe(0);
      expect(stability.after).not.toContain(freshName);
    } finally {
      await deleteIssuesQuietly(seed, cookie, seed.projectId, [created.id]);
    }
  }
);

test(
  specTitle(ROW_033, "bug: NEWFRONT-455 reconnecting refetches and shows newly published issues"),
  { tag: specTags(ROW_033) },
  async ({ driver, seed }) => {
    // Reconnect revalidation is left at the data layer's default, so an
    // offline/online cycle refetches the list and newly published issues
    // appear without any navigation (NEWFRONT-455). Intended: reconnecting
    // behaves like refocusing — no fetch, loaded data stays.
    const anchor = requireAnchor(seed);
    const cookie = await signInSession(seed.email, seed.password);
    const freshName = `SPC033 live ${uniqueSuffixForProjects()}`;
    await driver.spaceOpenBoard(anchor, "?board=list");
    const before = await driver.spaceBoardVisibleOrder();
    expect(before.length).toBeGreaterThan(0);
    const created = await createIssue(seed.workspaceSlug, seed.projectId, cookie, freshName);
    try {
      const { after } = await driver.spaceBoardReconnectStability();
      // Intended on web_new (bug NEWFRONT-455 fixed): reconnecting keeps
      // the loaded list like refocusing; the fresh issue stays hidden
      // until the visitor navigates or refilters.
      if (driver.target === "web_new") {
        expect(after).toEqual(before);
      } else {
        expect(after).toEqual([freshName, ...before]);
      }
    } finally {
      await deleteIssuesQuietly(seed, cookie, seed.projectId, [created.id]);
    }
  }
);
