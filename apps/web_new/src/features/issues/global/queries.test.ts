// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cursor paging for the workspace all-issues view (ISS-223): the read
// asks for the first window, follows the server's cursor until a page
// reports no successor, and the flattening step keeps row order.
import type { ApiClient, GlobalIssueListResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { GlobalIssueRow } from "@pidash/api-client";
import { flattenGlobalPages, GLOBAL_ISSUES_PAGE_SIZE, globalIssuesQueryOptions, globalTotalCount } from "./queries.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

/** A stable UUID for a short test id, so the row schema accepts it. */
function uuidFor(seed: string): string {
  let hex = "";
  for (const char of seed) hex += char.charCodeAt(0).toString(16).padStart(2, "0");
  const filled = `${hex}000000000000000000000000000000000000`.slice(0, 32);
  return `${filled.slice(0, 8)}-${filled.slice(8, 12)}-4${filled.slice(13, 16)}-8${filled.slice(17, 20)}-${filled.slice(20, 32)}`;
}

function row(id: string, sequence: number, projectId = PROJECT): GlobalIssueRow {
  return {
    id: id.length === 36 ? id : uuidFor(id),
    name: `Issue ${id}`,
    state_id: null,
    sort_order: sequence,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: sequence,
    project_id: projectId,
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
    state__group: null,
  };
}

function response(
  results: GlobalIssueRow[],
  options: { total?: number; nextCursor?: string; hasNext?: boolean } = {}
): GlobalIssueListResponse {
  const nextCursor = options.nextCursor ?? "";
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: options.total ?? results.length,
    next_cursor: nextCursor,
    prev_cursor: "",
    next_page_results: options.hasNext ?? nextCursor !== "",
    prev_page_results: false,
    count: results.length,
    total_results: options.total ?? results.length,
    results,
  };
}

function stubClient(pages: GlobalIssueListResponse[]): {
  client: ApiClient;
  calls: Array<Record<string, unknown>>;
} {
  const calls: Array<Record<string, unknown>> = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string, options?: { query?: Record<string, unknown> }) => {
      calls.push({ url, ...(options?.query ?? {}) });
      const next = pages[Math.min(calls.length - 1, pages.length - 1)];
      return next;
    }) as ApiClient["getJson"],
  } as ApiClient;
  return { client, calls };
}

describe("globalIssuesQueryOptions", () => {
  const options = globalIssuesQueryOptions({} as ApiClient, "acme");

  it("asks for the first window with no cursor and follows the server's cursor", async () => {
    const { client, calls } = stubClient([
      response([row("a", 1)], { total: 120, nextCursor: "100:1:0" }),
      response([row("b", 101)], { total: 120 }),
    ]);
    const run = globalIssuesQueryOptions(client, "acme");
    expect(run.initialPageParam).toBeNull();
    const queryFn = run.queryFn as (context: unknown) => Promise<GlobalIssueListResponse>;
    const first = await queryFn({ pageParam: null });
    expect(first.total_count).toBe(120);
    expect(calls[0]).toMatchObject({
      url: "/api/workspaces/acme/issues/",
      per_page: GLOBAL_ISSUES_PAGE_SIZE,
    });
    // The query-string builder always writes the key; the null value
    // means no cursor rides the first request.
    expect(calls[0]?.["cursor"]).toBeUndefined();
    const nextParam = run.getNextPageParam(first as never, [] as never, null as never, [] as never) as unknown;
    expect(nextParam).toBe("100:1:0");
    await queryFn({ pageParam: nextParam });
    expect(calls[1]).toMatchObject({ cursor: "100:1:0", per_page: GLOBAL_ISSUES_PAGE_SIZE });
  });

  it("ends the chain on an empty cursor or a false next-page flag", () => {
    expect(options.getNextPageParam(response([row("a", 1)]), [] as never, null as never, [] as never)).toBeNull();
    expect(
      options.getNextPageParam(
        response([row("a", 1)], { nextCursor: "5:1:0", hasNext: false }),
        [] as never,
        null as never,
        [] as never
      )
    ).toBeNull();
    expect(
      options.getNextPageParam(
        response([row("a", 1)], { nextCursor: "   ", hasNext: true }),
        [] as never,
        null as never,
        [] as never
      )
    ).toBeNull();
  });

  it("keys the cache on the workspace alone so a deep link shares the plain entry", () => {
    expect(options.queryKey).toEqual(["ws", "acme", "issues", "global"]);
  });
});

describe("globalTotalCount", () => {
  it("reads the first page's server total and falls back to zero", () => {
    expect(globalTotalCount(undefined)).toBe(0);
    expect(globalTotalCount({ pages: [] })).toBe(0);
    expect(globalTotalCount({ pages: [response([row("a", 1)], { total: 42 })] })).toBe(42);
  });
});

describe("flattenGlobalPages", () => {
  it("returns nothing before the first page", () => {
    expect(flattenGlobalPages(undefined)).toEqual([]);
  });

  it("keeps page order and drops a repeated row id", () => {
    const rows = flattenGlobalPages({
      pages: [response([row("a", 1), row("b", 2)]), response([row("b", 2), row("c", 3)])],
    });
    expect(rows.map((entry) => entry.name)).toEqual(["Issue a", "Issue b", "Issue c"]);
  });
});
