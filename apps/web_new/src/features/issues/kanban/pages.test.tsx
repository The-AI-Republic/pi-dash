// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board reads (parity/specs/issues.md §5, ISS-028/029): the grouped and
// sub-grouped endpoints, their page sizes and grouping parameters, the
// project/cycle/module paths, the key axes and envelope shaping.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createClient, type IssueListItem } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { resetAppClient } from "../../../core/api/client.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { boardQueryKey, isGrouped, isSubGrouped, readBoard, toBoard, useBoard, type BoardReadInput } from "./pages.js";

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
