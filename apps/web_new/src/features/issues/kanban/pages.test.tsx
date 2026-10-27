// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board reads (parity/specs/issues.md §5, ISS-028/029, ISS-042): the grouped
// and sub-grouped endpoints, their page sizes and grouping parameters,
// the project/cycle/module paths, the key axes, envelope shaping, and
// per-column/per-cell later pages.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createClient, type IssueListItem } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { resetAppClient } from "../../../core/api/client.js";
import type { IssuesEntity } from "../prefs/entities.js";
import {
  boardCellKey,
  boardCellOf,
  boardQueryKey,
  isGrouped,
  isSubGrouped,
  readBoard,
  readLabelRecovery,
  toBoard,
  useBoard,
  type BoardReadInput,
} from "./pages.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";
const STATE = "523e4567-e89b-12d3-a456-426614174004";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const envelope = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 1,
  next_cursor: "",
  prev_cursor: "",
  next_page_results: false,
  prev_page_results: false,
  count: 1,
  total_results: 1,
  results: [row()],
};

const groupedEnvelope = {
  ...envelope,
  grouped_by: "state_id",
  results: { [STATE]: { results: [row()], total_results: 4 } },
};

const subEnvelope = {
  ...envelope,
  grouped_by: "state_id",
  sub_grouped_by: "labels__id",
  results: { [STATE]: { results: { None: { results: [row()], total_results: 1 } }, total_results: 4 } },
};

function recordingClient(body: Record<string, unknown>) {
  const calls: Array<{ method: string; url: string }> = [];
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url, init) => {
      calls.push({ method: init.method ?? "GET", url });
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
    },
  });
  return { client, calls };
}

function input(overrides: Partial<BoardReadInput> = {}): BoardReadInput {
  return {
    entity: { scope: "project", projectId: PROJECT },
    groupBy: "state",
    subGroupBy: null,
    orderBy: "sort_order",
    showEmpty: true,
    groupLabels: new Map([[STATE, "Todo"]]),
    subLabels: new Map(),
    ...overrides,
  };
}

afterEach(() => resetAppClient());

describe("readBoard", () => {
  it.each([
    ["project", { scope: "project", projectId: PROJECT } as const, "/issues/"],
    ["cycle", { scope: "cycle", projectId: PROJECT, cycleId: CYCLE } as const, `/cycles/${CYCLE}/cycle-issues/`],
    ["module", { scope: "module", projectId: PROJECT, moduleId: MODULE } as const, `/modules/${MODULE}/issues/`],
  ] as const)("reads the grouped %s board at 30 per page", async (_label, entity, suffix) => {
    const { client, calls } = recordingClient(groupedEnvelope);
    await readBoard(client, "acme", input({ entity }));
    const url = new URL(calls[0]!.url);
    expect(url.pathname).toBe(`/api/workspaces/acme/projects/${PROJECT}${suffix}`);
    expect(url.searchParams.get("per_page")).toBe("30");
    expect(url.searchParams.get("group_by")).toBe("state_id");
    expect(url.searchParams.get("sub_group_by")).toBeNull();
    expect(url.searchParams.get("order_by")).toBe("sort_order");
  });

  it("reads ten per cell when a sub-group is active", async () => {
    const { client, calls } = recordingClient(subEnvelope);
    await readBoard(client, "acme", input({ subGroupBy: "labels" }));
    const url = new URL(calls[0]!.url);
    expect(url.pathname).toBe(`/api/workspaces/acme/projects/${PROJECT}/issues/`);
    expect(url.searchParams.get("per_page")).toBe("10");
    expect(url.searchParams.get("group_by")).toBe("state_id");
    expect(url.searchParams.get("sub_group_by")).toBe("labels__id");
  });

  it("ignores a stored sub-group that repeats the group", async () => {
    const { client, calls } = recordingClient(groupedEnvelope);
    await readBoard(client, "acme", input({ subGroupBy: "state" }));
    const url = new URL(calls[0]!.url);
    expect(url.searchParams.get("per_page")).toBe("30");
    expect(url.searchParams.get("sub_group_by")).toBeNull();
  });

  it("reads the flat list at the project path without grouping", async () => {
    const { client, calls } = recordingClient(envelope);
    await readBoard(client, "acme", input({ groupBy: null }));
    const url = new URL(calls[0]!.url);
    expect(url.searchParams.get("group_by")).toBeNull();
    expect(url.searchParams.get("per_page")).toBe("30");
  });

  it("reads the flat cycle and module lists at their own paths", async () => {
    for (const [entity, suffix] of [
      [{ scope: "cycle", projectId: PROJECT, cycleId: CYCLE }, `/cycles/${CYCLE}/cycle-issues/`],
      [{ scope: "module", projectId: PROJECT, moduleId: MODULE }, `/modules/${MODULE}/issues/`],
    ] as const) {
      const { client, calls } = recordingClient(envelope);
      await readBoard(client, "acme", input({ entity, groupBy: null }));
      const url = new URL(calls[0]!.url);
      expect(url.pathname).toBe(`/api/workspaces/acme/projects/${PROJECT}${suffix}`);
      expect(url.searchParams.get("group_by")).toBeNull();
    }
  });

  it("sends the sub-issue flag and rejects entities without a project", async () => {
    const { client, calls } = recordingClient(groupedEnvelope);
    await readBoard(client, "acme", input({ showSubIssues: true }));
    expect(new URL(calls[0]!.url).searchParams.get("sub_issue")).toBe("true");
    await expect(readBoard(client, "acme", input({ entity: { scope: "workspace" } }))).rejects.toThrow(
      "project-scoped"
    );
  });
});

describe("readLabelRecovery", () => {
  it("rejects an ungrouped board and folds pages without duplicating rows", async () => {
    const first = {
      ...envelope,
      grouped_by: "state_id",
      total_count: 2,
      next_cursor: "100:1:0",
      next_page_results: true,
      results: { [STATE]: { results: [row()], total_results: 2 } },
    };
    const second = {
      ...envelope,
      grouped_by: "state_id",
      total_count: 2,
      results: { [STATE]: { results: [row(), row({ id: "123e4567-e89b-12d3-a456-426614174099" })], total_results: 2 } },
    };
    const pages: Record<string, unknown> = { "100:1:0": second };
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        const cursor = new URL(url).searchParams.get("cursor") ?? "";
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(pages[cursor] ?? first),
        };
      },
    });
    await expect(readLabelRecovery(client, "acme", input({ groupBy: null }))).rejects.toThrow("grouped board");
    await expect(readLabelRecovery(client, "acme", input({ entity: { scope: "workspace" } }))).rejects.toThrow(
      "project-scoped"
    );
    const recovered = await readLabelRecovery(client, "acme", input());
    expect(recovered.results[STATE]?.results).toHaveLength(2);
    expect(recovered.results[STATE]?.total_results).toBe(2);
  });

  it("stops when the server repeats its next cursor", async () => {
    const page = {
      ...envelope,
      grouped_by: "state_id",
      next_cursor: "100:1:0",
      next_page_results: true,
      results: { [STATE]: { results: [row()], total_results: 1 } },
    };
    const { client } = recordingClient(page);
    const recovered = await readLabelRecovery(client, "acme", input());
    expect(recovered.results[STATE]?.results).toHaveLength(1);
  });
});

describe("label recovery", () => {
  it("walks the grouped fallback and folds an omitted unlabeled row into None", async () => {
    const ghost = row({ id: "123e4567-e89b-12d3-a456-426614174010", label_ids: [] });
    const kept = row({
      id: "123e4567-e89b-12d3-a456-426614174011",
      label_ids: ["423e4567-e89b-12d3-a456-426614174003"],
    });
    const plain = row({ id: "123e4567-e89b-12d3-a456-426614174012", label_ids: [] });
    // The server drops the ghost from the lanes and from every count, so
    // the envelope claims two rows and sums to two (NEWFRONT-158).
    const subEnvelope = {
      ...envelope,
      grouped_by: "state_id",
      sub_grouped_by: "labels__id",
      total_count: 2,
      total_results: 2,
      results: {
        [STATE]: {
          results: {
            "423e4567-e89b-12d3-a456-426614174003": { results: [kept], total_results: 1 },
            None: { results: [plain], total_results: 1 },
          },
          total_results: 2,
        },
      },
    };
    const fallback = {
      ...envelope,
      grouped_by: "state_id",
      total_count: 3,
      results: { [STATE]: { results: [kept, ghost, plain], total_results: 3 } },
    };
    const calls: string[] = [];
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url) => {
        calls.push(url);
        const sub = new URL(url).searchParams.get("sub_group_by");
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(sub === null ? fallback : subEnvelope),
        };
      },
    });
    const queryClient = boardClient();
    const { result } = renderHook(() => useBoard(client, "acme", input({ subGroupBy: "labels" })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board?.lanes).toHaveLength(2));
    const board = result.current.board;
    expect(board?.lanes.map((lane) => [lane.key, lane.total])).toEqual([
      ["423e4567-e89b-12d3-a456-426614174003", 1],
      ["None", 2],
    ]);
    expect(board?.lanes[1]?.cells[STATE]?.rows.map((entry) => entry.id)).toEqual([plain.id, ghost.id]);
    expect(board?.lanes[1]?.cells[STATE]?.total).toBe(2);
    expect(board?.columns[0]?.total).toBe(3);
    expect(board?.total).toBe(3);
    expect(calls.some((url) => new URL(url).searchParams.get("sub_group_by") === null)).toBe(true);
  });

  it("does not request a fallback for non-label sub-grouping", async () => {
    const { client, calls } = recordingClient(subEnvelope);
    const queryClient = boardClient();
    const { result } = renderHook(() => useBoard(client, "acme", input({ subGroupBy: "assignees" })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(calls).toHaveLength(1);
  });
});

describe("boardQueryKey", () => {
  it("changes with every axis that changes the envelope", () => {
    const base = boardQueryKey("acme", input());
    expect(base).toEqual(boardQueryKey("acme", input()));
    expect(boardQueryKey("acme", input({ groupBy: "priority" }))).not.toEqual(base);
    expect(boardQueryKey("acme", input({ subGroupBy: "labels" }))).not.toEqual(base);
    expect(boardQueryKey("acme", input({ orderBy: "-created_at" }))).not.toEqual(base);
    expect(boardQueryKey("acme", input({ showEmpty: false }))).not.toEqual(base);
    expect(boardQueryKey("acme", input({ showSubIssues: true }))).not.toEqual(base);
  });

  it("names the flat and ungrouped axes in the key", () => {
    expect(boardQueryKey("acme", input({ groupBy: null }))).toContain("flat");
    expect(boardQueryKey("acme", input({ subGroupBy: "labels" }))).toContain("labels");
  });

  it("collapses a repeated sub-group to the same key", () => {
    expect(boardQueryKey("acme", input({ subGroupBy: "state" }))).toEqual(boardQueryKey("acme", input()));
  });
});

describe("envelope shaping", () => {
  it("recognizes each envelope shape", () => {
    expect(isSubGrouped(subEnvelope as never)).toBe(true);
    expect(isGrouped(subEnvelope as never)).toBe(false);
    expect(isGrouped(groupedEnvelope as never)).toBe(true);
    expect(isSubGrouped(groupedEnvelope as never)).toBe(false);
    expect(isGrouped(envelope as never)).toBe(false);
  });

  it("titles a grouped board from the caller's label map", () => {
    const board = toBoard(groupedEnvelope as never, input());
    expect(board.columns).toEqual([{ key: STATE, title: "Todo", total: 4 }]);
    expect(board.subGrouped).toBe(false);
  });

  it("shapes a sub-grouped board with lanes", () => {
    const board = toBoard(subEnvelope as never, input({ subGroupBy: "labels" }));
    expect(board.subGrouped).toBe(true);
    expect(board.lanes.map((lane) => lane.title)).toEqual(["None"]);
  });

  it("shapes a flat board into its single column", () => {
    const board = toBoard(envelope as never, input({ groupBy: null }));
    expect(board.columns).toHaveLength(1);
    expect(board.lanes[0]?.cells["all"]?.rows).toHaveLength(1);
  });
});

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

describe("useBoard", () => {
  it("reports the pending read, then the shaped board", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { client } = recordingClient(groupedEnvelope);
    const entity: IssuesEntity = { scope: "project", projectId: PROJECT };
    const { result } = renderHook(() => useBoard(client, "acme", input({ entity })), {
      wrapper: wrapper(queryClient),
    });
    expect(result.current.board).toBeNull();
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.board?.columns[0]?.title).toBe("Todo");
  });
});

function many(prefix: string, count: number, start = 4000): IssueListItem[] {
  return Array.from({ length: count }, (_, index) =>
    row({
      id: `223e4567-e89b-12d3-a456-42661417${String(start + index).slice(-4)}`,
      name: `${prefix} ${index}`,
      sequence_id: 100 + index,
    })
  );
}

function extraRow(index: number): IssueListItem {
  return row({
    id: `223e4567-e89b-12d3-a456-42661417${String(5000 + index).slice(-4)}`,
    name: `extra ${index}`,
    sequence_id: 200 + index,
  });
}

function groupedPage(rows: IssueListItem[], total: number): Record<string, unknown> {
  return {
    ...envelope,
    grouped_by: "state_id",
    total_count: total,
    results: { [STATE]: { results: rows, total_results: total } },
  };
}

function subPage(rows: IssueListItem[], total: number): Record<string, unknown> {
  return {
    ...envelope,
    grouped_by: "state_id",
    sub_grouped_by: "labels__id",
    total_count: total,
    results: {
      [STATE]: { results: { None: { results: rows, total_results: total } }, total_results: total },
    },
  };
}

function flatPage(rows: IssueListItem[], total: number, nextCursor: string): Record<string, unknown> {
  return {
    ...envelope,
    total_count: total,
    count: rows.length,
    total_results: total,
    next_cursor: nextCursor,
    next_page_results: nextCursor !== "",
    results: rows,
  };
}

function pagingClient(first: Record<string, unknown>, pages: Record<string, Record<string, unknown>>) {
  const calls: string[] = [];
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      calls.push(url);
      const cursor = new URL(url).searchParams.get("cursor") ?? "";
      const body = cursor === "" ? first : (pages[cursor] ?? first);
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
    },
  });
  return { client, calls };
}

function deferredClient(first: Record<string, unknown>, second: Record<string, unknown>) {
  const calls: string[] = [];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const client = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => {
      calls.push(url);
      const cursor = new URL(url).searchParams.get("cursor") ?? "";
      const body = cursor === "" ? first : second;
      if (cursor !== "") await gate;
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
    },
  });
  return { client, calls, release };
}

/** Board reads that ask for a sub-group, in request order. */
function subGroupCalls(calls: readonly string[]): string[] {
  return calls.filter((url) => new URL(url).searchParams.get("sub_group_by") === "labels__id");
}

function boardClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

describe("boardCellKey and boardCellOf", () => {
  it("keys a cell by its lane and column", () => {
    expect(boardCellKey(STATE, "bug")).toBe(JSON.stringify(["bug", STATE]));
  });

  it("reads flat and swimlane cells and misses cleanly", () => {
    expect(boardCellOf(null, STATE, null)).toBeNull();
    const board = toBoard(groupedPage(many("g", 2), 2) as never, input());
    expect(boardCellOf(board, STATE, null)?.rows).toHaveLength(2);
    expect(boardCellOf(board, "missing", null)).toBeNull();
    expect(boardCellOf(board, STATE, "missing")).toBeNull();
    expect(boardCellOf({ ...board, lanes: [] }, STATE, null)).toBeNull();
    const sub = toBoard(subPage(many("s", 1), 1) as never, input({ subGroupBy: "labels" }));
    expect(boardCellOf(sub, STATE, "None")?.rows).toHaveLength(1);
  });
});

describe("board pagination", () => {
  it("sends the cursor on later grouped and flat reads", async () => {
    const { client, calls } = recordingClient(groupedEnvelope);
    await readBoard(client, "acme", input(), "30:1:0");
    expect(new URL(calls[0]!.url).searchParams.get("cursor")).toBe("30:1:0");
    const flat = recordingClient(envelope);
    await readBoard(flat.client, "acme", input({ groupBy: null }), "30:1:0");
    expect(new URL(flat.calls[0]!.url).searchParams.get("cursor")).toBe("30:1:0");
  });

  it("pages a grouped column at its own row count without refetching cursors", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(groupedPage(many("g", 30), 32), {
      "30:1:0": groupedPage([extraRow(0), extraRow(1)], 32),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input()), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.columnHasMore(STATE)).toBe(true);
    expect(result.current.columnHasMore("missing")).toBe(false);
    act(() => result.current.loadMoreColumn(STATE));
    await waitFor(() => expect(result.current.columnHasMore(STATE)).toBe(false));
    expect(boardCellOf(result.current.board, STATE, null)?.rows).toHaveLength(32);
    expect(calls).toHaveLength(2);
    expect(new URL(calls[1]!).searchParams.get("cursor")).toBe("30:1:0");
    act(() => result.current.loadMoreColumn(STATE));
    act(() => result.current.loadMoreColumn("missing"));
    expect(calls).toHaveLength(2);
  });

  it("leaves other columns alone when one column pages deeper", async () => {
    const OTHER = "623e4567-e89b-12d3-a456-426614174005";
    const twoColumnPage = (
      aRows: IssueListItem[],
      aTotal: number,
      bRows: IssueListItem[],
      bTotal: number
    ): Record<string, unknown> => ({
      ...envelope,
      grouped_by: "state_id",
      total_count: aTotal + bTotal,
      results: {
        [STATE]: { results: aRows, total_results: aTotal },
        [OTHER]: { results: bRows, total_results: bTotal },
      },
    });
    const queryClient = boardClient();
    const { client, calls } = pagingClient(twoColumnPage(many("a", 30), 32, many("b", 10, 4100), 30), {
      "30:1:0": twoColumnPage([extraRow(0), extraRow(1)], 32, [extraRow(4), extraRow(5)], 30),
      "30:0:0": twoColumnPage(many("a", 30), 32, [extraRow(2), extraRow(3)], 30),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input()), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    act(() => result.current.loadMoreColumn(STATE));
    await waitFor(() => expect(boardCellOf(result.current.board, STATE, null)?.rows).toHaveLength(32));
    expect(boardCellOf(result.current.board, OTHER, null)?.rows).toHaveLength(10);
    expect(result.current.columnHasMore(OTHER)).toBe(true);
    act(() => result.current.loadMoreColumn(OTHER));
    await waitFor(() => expect(boardCellOf(result.current.board, OTHER, null)?.rows).toHaveLength(12));
    expect(calls).toHaveLength(3);
  });

  it("pages a sub-grouped cell behind its own cursor", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(subPage(many("s", 10), 12), {
      "10:1:0": subPage([extraRow(0), extraRow(1)], 12),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input({ subGroupBy: "labels" })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.cellHasMore(STATE, "None")).toBe(true);
    expect(result.current.cellHasMore(STATE, "missing")).toBe(false);
    act(() => result.current.loadMoreCell(STATE, "None"));
    await waitFor(() => expect(result.current.cellHasMore(STATE, "None")).toBe(false));
    expect(boardCellOf(result.current.board, STATE, "None")?.rows).toHaveLength(12);
    const pages = subGroupCalls(calls);
    expect(pages).toHaveLength(2);
    expect(new URL(pages[1]!).searchParams.get("cursor")).toBe("10:1:0");
    act(() => result.current.loadMoreCell(STATE, "None"));
    act(() => result.current.loadMoreCell(STATE, "missing"));
    expect(subGroupCalls(calls)).toHaveLength(2);
  });

  it("follows the flat cursor chain on the single column", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(flatPage(many("f", 30), 32, "30:1:0"), {
      "30:1:0": flatPage([extraRow(0), extraRow(1)], 32, ""),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input({ groupBy: null })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.columnHasMore("all")).toBe(true);
    act(() => result.current.loadMoreColumn("all"));
    await waitFor(() => expect(result.current.columnHasMore("all")).toBe(false));
    expect(boardCellOf(result.current.board, "all", null)?.rows).toHaveLength(32);
    expect(calls).toHaveLength(2);
    act(() => result.current.loadMoreColumn("all"));
    expect(calls).toHaveLength(2);
  });

  it("holds the flat column while its chain ends or loses its cursor", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(flatPage(many("f", 2), 3, ""), {});
    const { result } = renderHook(() => useBoard(client, "acme", input({ groupBy: null })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.columnHasMore("all")).toBe(true);
    act(() => result.current.loadMoreColumn("all"));
    expect(calls).toHaveLength(1);
  });

  it("ignores a grouped cursor the server already answered", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(groupedPage(many("g", 30), 32), {
      "30:1:0": groupedPage([], 32),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input()), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    act(() => result.current.loadMoreColumn(STATE));
    await waitFor(() => expect(calls).toHaveLength(2));
    expect(result.current.columnHasMore(STATE)).toBe(true);
    act(() => result.current.loadMoreColumn(STATE));
    expect(calls).toHaveLength(2);
  });

  it("ignores a cell cursor the server already answered", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(subPage(many("s", 10), 12), {
      "10:1:0": subPage([], 12),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input({ subGroupBy: "labels" })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    act(() => result.current.loadMoreCell(STATE, "None"));
    await waitFor(() => expect(subGroupCalls(calls)).toHaveLength(2));
    expect(result.current.cellHasMore(STATE, "None")).toBe(true);
    act(() => result.current.loadMoreCell(STATE, "None"));
    expect(subGroupCalls(calls)).toHaveLength(2);
  });

  it("holds a flat cursor the server repeats", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(flatPage(many("f", 30), 60, "30:1:0"), {
      "30:1:0": flatPage([], 60, "30:1:0"),
    });
    const { result } = renderHook(() => useBoard(client, "acme", input({ groupBy: null })), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.columnFetching("all")).toBe(false);
    act(() => result.current.loadMoreColumn("all"));
    await waitFor(() => expect(calls).toHaveLength(2));
    expect(result.current.columnHasMore("all")).toBe(true);
    act(() => result.current.loadMoreColumn("all"));
    expect(calls).toHaveLength(2);
  });

  it("reports per-owner fetching while a later page flies", async () => {
    const queryClient = boardClient();
    const { client, release } = deferredClient(groupedPage(many("g", 30), 32), groupedPage([extraRow(0)], 32));
    const { result } = renderHook(() => useBoard(client, "acme", input()), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.board).not.toBeNull());
    expect(result.current.columnFetching(STATE)).toBe(false);
    act(() => result.current.loadMoreColumn(STATE));
    await waitFor(() => expect(result.current.columnFetching(STATE)).toBe(true));
    expect(result.current.isFetching).toBe(true);
    expect(result.current.columnFetching("other")).toBe(false);
    expect(result.current.cellFetching(STATE, "None")).toBe(false);
    act(() => release());
    await waitFor(() => expect(result.current.columnFetching(STATE)).toBe(false));
  });

  it("resets asked cursors when the read changes", async () => {
    const queryClient = boardClient();
    const { client, calls } = pagingClient(groupedPage(many("g", 30), 32), {
      "30:1:0": groupedPage([extraRow(0)], 32),
    });
    const { result, rerender } = renderHook(
      ({ groupBy }: { groupBy: string | null }) => useBoard(client, "acme", input({ groupBy })),
      {
        wrapper: wrapper(queryClient),
        initialProps: { groupBy: "state" as string | null },
      }
    );
    await waitFor(() => expect(result.current.board).not.toBeNull());
    act(() => result.current.loadMoreColumn(STATE));
    await waitFor(() => expect(calls).toHaveLength(2));
    rerender({ groupBy: "priority" });
    await waitFor(() => expect(calls).toHaveLength(3));
    expect(new URL(calls[2]!).searchParams.get("group_by")).toBe("priority");
    expect(calls.filter((url) => new URL(url).searchParams.has("cursor"))).toHaveLength(1);
  });

  it("answers empty while the first page is pending", () => {
    const queryClient = boardClient();
    const { client } = pagingClient(groupedPage(many("g", 1), 1), {});
    const { result } = renderHook(() => useBoard(client, "acme", input()), {
      wrapper: wrapper(queryClient),
    });
    expect(result.current.board).toBeNull();
    expect(result.current.columnHasMore(STATE)).toBe(false);
    expect(result.current.cellHasMore(STATE, "None")).toBe(false);
    expect(result.current.columnFetching(STATE)).toBe(false);
    expect(result.current.cellFetching(STATE, "None")).toBe(false);
    act(() => result.current.loadMoreColumn(STATE));
    act(() => result.current.loadMoreCell(STATE, "None"));
  });
});
