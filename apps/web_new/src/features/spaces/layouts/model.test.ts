// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssue, SpaceBoardIssuePage, SpaceBoardState } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  appendGroupPage,
  enabledSpaceLayouts,
  findGroup,
  firstPageGroups,
  groupHasNextPage,
  orderedSpaceStates,
  resolveSpaceLayout,
  SPACE_BOARD_ORDER,
  SPACE_BOARD_PAGE_SIZE,
  SPACE_FIRST_GROUP_CURSOR,
  spaceBoardQuery,
  spaceFilterSignature,
  type SpaceIssueGroup,
} from "./model.js";

const TODO = "22222222-2222-4222-8222-222222222222";
const DONE = "33333333-3333-4333-8333-333333333333";
const EMPTY = "44444444-4444-4444-8444-444444444444";
const ID_A = "11111111-1111-4111-8111-111111111111";
const ID_B = "55555555-5555-4555-8555-555555555555";

function state(id: string, name: string, sequence: number): SpaceBoardState {
  return { id, name, color: "#000", group: "unstarted", sequence };
}

const states: SpaceBoardState[] = [state(DONE, "Done", 3), state(EMPTY, "Empty", 2), state(TODO, "Todo", 1)];

function issue(id: string, sequenceId: number, stateId: string): SpaceBoardIssue {
  return {
    id,
    name: `Issue ${sequenceId}`,
    state_id: stateId,
    sort_order: 1,
    estimate_point: null,
    priority: null,
    start_date: null,
    target_date: null,
    sequence_id: sequenceId,
    project_id: ID_A,
    parent_id: null,
    created_by: null,
    state__group: "unstarted",
    cycle_id: null,
    assignee_ids: [],
    label_ids: [],
    module_ids: [],
    vote_items: [],
    reaction_items: [],
  };
}

function page(overrides: Partial<SpaceBoardIssuePage> = {}): SpaceBoardIssuePage {
  return {
    grouped_by: "state_id",
    total_count: 2,
    next_cursor: null,
    next_page_results: false,
    results: {},
    ...overrides,
  };
}

describe("enabled layouts (SPC-020)", () => {
  it("offers the renderable intersection in list-then-board order", () => {
    expect(enabledSpaceLayouts({ list: true, kanban: true, calendar: true, gantt: true, spreadsheet: true })).toEqual([
      "list",
      "kanban",
    ]);
    expect(enabledSpaceLayouts({ kanban: true, list: false })).toEqual(["kanban"]);
    expect(enabledSpaceLayouts({ list: true, kanban: false })).toEqual(["list"]);
  });

  it("never offers a layout outside the renderable pair, however the payload spells it", () => {
    expect(enabledSpaceLayouts({ calendar: true, gantt: true, spreadsheet: true, list: false })).toEqual(["list"]);
    expect(enabledSpaceLayouts({ list: "yes" as unknown as boolean })).toEqual(["list"]);
  });

  it("falls back to list when the publisher enabled nothing renderable", () => {
    expect(enabledSpaceLayouts({})).toEqual(["list"]);
  });
});

describe("layout settlement (SPC-020/022)", () => {
  it("keeps a requested layout the board offers", () => {
    expect(resolveSpaceLayout("kanban", ["list", "kanban"])).toBe("kanban");
    expect(resolveSpaceLayout("list", ["list", "kanban"])).toBe("list");
  });

  it("settles on the first offered layout when the request is missing or not offered", () => {
    expect(resolveSpaceLayout(undefined, ["list", "kanban"])).toBe("list");
    expect(resolveSpaceLayout("list", ["kanban"])).toBe("kanban");
    expect(resolveSpaceLayout("kanban", ["list"])).toBe("list");
  });

  it("still names a layout when a caller offers nothing", () => {
    expect(resolveSpaceLayout(undefined, [])).toBe("list");
    expect(resolveSpaceLayout("kanban", [])).toBe("list");
  });
});

describe("filter signature", () => {
  it("is stable across facet order and distinguishes every combination", () => {
    expect(spaceFilterSignature({})).toBe("||");
    expect(spaceFilterSignature({ state: [ID_A, ID_B] })).toBe(`${ID_A},${ID_B}||`);
    expect(spaceFilterSignature({ priority: ["high"], labels: [ID_A] })).toBe(`|high|${ID_A}`);
    expect(spaceFilterSignature({ state: [ID_A] })).not.toBe(spaceFilterSignature({ state: [ID_B] }));
  });
});

describe("board query (SPC-026/036)", () => {
  it("asks for the first grouped page at the literal first cursor", () => {
    expect(SPACE_BOARD_PAGE_SIZE).toBe(50);
    expect(SPACE_FIRST_GROUP_CURSOR).toBe("50:0:0");
    expect(SPACE_BOARD_ORDER).toBe("-created_at");
    expect(spaceBoardQuery({})).toEqual({
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
      orderBy: "-created_at",
    });
  });

  it("carries every address facet on the first fetch", () => {
    expect(spaceBoardQuery({ state: [ID_A], priority: ["urgent", "none"], labels: [ID_B] })).toEqual({
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:0:0",
      orderBy: "-created_at",
      state: [ID_A],
      priority: ["urgent", "none"],
      labels: [ID_B],
    });
  });

  it("asks one group for a later page with that group's own cursor", () => {
    expect(spaceBoardQuery({ state: [ID_A], priority: ["high"] }, "50:1:0", TODO)).toEqual({
      groupBy: "state_id",
      perPage: 50,
      cursor: "50:1:0",
      orderBy: "-created_at",
      state: [TODO],
      priority: ["high"],
    });
  });
});

describe("group ordering and the first page (SPC-023/024)", () => {
  it("orders states by sequence and breaks ties by name", () => {
    const tied = [state(ID_B, "Beta", 2), state(ID_A, "Alpha", 2), state(TODO, "Todo", 1)];
    expect(orderedSpaceStates(tied).map((candidate) => candidate.name)).toEqual(["Todo", "Alpha", "Beta"]);
    expect(tied.map((candidate) => candidate.id)).toEqual([ID_B, ID_A, TODO]);
  });

  it("builds one group per workflow state, empty ones included, with server counts", () => {
    const groups = firstPageGroups(
      states,
      page({
        total_count: 3,
        results: {
          [TODO]: { results: [issue(ID_A, 1, TODO), issue(ID_B, 2, TODO)], total_results: 7 },
          [DONE]: { results: [issue("66666666-6666-4666-8666-666666666666", 3, DONE)], total_results: 1 },
        },
      })
    );
    expect(groups.map((group) => group.state.name)).toEqual(["Todo", "Empty", "Done"]);
    expect(groups.map((group) => group.total)).toEqual([7, 0, 1]);
    expect(groups.map((group) => group.issues.length)).toEqual([2, 0, 1]);
    expect(groups[1]).toMatchObject({ stateId: EMPTY, issues: [], total: 0, nextCursor: null });
  });

  it("uses the row count when a bucket carries no total", () => {
    const groups = firstPageGroups(states, {
      ...page(),
      results: { [TODO]: { results: [issue(ID_A, 1, TODO)], total_results: undefined as unknown as number } },
    });
    expect(groups[0]?.total).toBe(1);
  });

  it("offers the shared first cursor only to groups with unloaded rows", () => {
    const buckets = {
      [TODO]: { results: [issue(ID_A, 1, TODO)], total_results: 60 },
      [DONE]: { results: [issue(ID_B, 2, DONE)], total_results: 1 },
    };
    const withNext = firstPageGroups(
      states,
      page({ results: buckets, next_cursor: "50:1:0", next_page_results: true })
    );
    expect(withNext.map((group) => group.nextCursor)).toEqual(["50:1:0", null, null]);
  });

  it("offers no next page when the response says there is none or carries no cursor", () => {
    const buckets = { [TODO]: { results: [issue(ID_A, 1, TODO)], total_results: 60 } };
    const drained = firstPageGroups(
      states,
      page({ results: buckets, next_cursor: "50:1:0", next_page_results: false })
    );
    expect(drained.map((group) => group.nextCursor)).toEqual([null, null, null]);
    const cursorless = firstPageGroups(states, page({ results: buckets, next_cursor: null, next_page_results: true }));
    expect(cursorless.map((group) => group.nextCursor)).toEqual([null, null, null]);
  });
});

function groupOf(overrides: Partial<SpaceIssueGroup> = {}): SpaceIssueGroup {
  return {
    stateId: TODO,
    state: state(TODO, "Todo", 1),
    issues: [issue(ID_A, 1, TODO)],
    total: 3,
    nextCursor: "50:1:0",
    ...overrides,
  };
}

describe("appending a later page (SPC-026)", () => {
  it("appends the new rows, keeps the server total and retires the cursor at the end", () => {
    const next = appendGroupPage(
      groupOf(),
      page({
        results: { [TODO]: { results: [issue(ID_B, 2, TODO)], total_results: 3 } },
        next_cursor: "50:2:0",
        next_page_results: false,
      })
    );
    expect(next.issues.map((candidate) => candidate.id)).toEqual([ID_A, ID_B]);
    expect(next.total).toBe(3);
    expect(next.nextCursor).toBeNull();
    expect(next.stateId).toBe(TODO);
  });

  it("keeps paging while the group still has more rows than it shows", () => {
    const next = appendGroupPage(
      groupOf({ total: 120 }),
      page({
        results: { [TODO]: { results: [issue(ID_B, 2, TODO)], total_results: 120 } },
        next_cursor: "50:2:0",
        next_page_results: true,
      })
    );
    expect(next.issues).toHaveLength(2);
    expect(next.nextCursor).toBe("50:2:0");
  });

  it("drops rows already shown so a retried page cannot duplicate cards", () => {
    const current = groupOf();
    const next = appendGroupPage(
      current,
      page({ results: { [TODO]: { results: [issue(ID_A, 1, TODO), issue(ID_A, 1, TODO)], total_results: 3 } } })
    );
    expect(next.issues).toHaveLength(1);
    expect(next.issues).toBe(current.issues);
  });

  it("keeps the group's own count and shape when the page carries no bucket", () => {
    const current = groupOf();
    const next = appendGroupPage(current, page({ results: {}, next_cursor: "50:2:0", next_page_results: true }));
    expect(next.issues).toBe(current.issues);
    expect(next.total).toBe(3);
    // Nothing landed, so the group still has rows missing and keeps paging.
    expect(next.nextCursor).toBe("50:2:0");
  });

  it("takes the page's total when the server sends one", () => {
    const next = appendGroupPage(
      groupOf(),
      page({ results: { [TODO]: { results: [issue(ID_B, 2, TODO)], total_results: 9 } } })
    );
    expect(next.total).toBe(9);
  });
});

describe("group lookup", () => {
  it("finds a group by state and reports whether it has another page", () => {
    const groups = [groupOf(), groupOf({ stateId: DONE, nextCursor: null })];
    expect(findGroup(groups, DONE)?.stateId).toBe(DONE);
    expect(findGroup(groups, EMPTY)).toBeUndefined();
    expect(groups.map(groupHasNextPage)).toEqual([true, false]);
  });
});
