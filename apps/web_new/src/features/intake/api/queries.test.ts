// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import type { IntakeIssueListResponse, IntakeIssueRow } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { intakeKeys } from "./keys.js";
import {
  INTAKE_PAGE_SIZE,
  intakeListQueryOptions,
  intakeRows,
  intakeTotal,
  listStatuses,
  prefetchIntakeList,
} from "./queries.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function row(id: string, name: string): IntakeIssueRow {
  return {
    id: `intake-${id}`,
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    source: null,
    created_by: null,
    issue: {
      id,
      name,
      priority: "none",
      sequence_id: 1,
      project_id: PROJECT,
      created_at: "2026-10-01T00:00:00Z",
      label_ids: [],
      created_by: null,
    },
  };
}

function page(rows: IntakeIssueRow[], total: number, next: string | undefined): IntakeIssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: total,
    next_cursor: next ?? "",
    prev_cursor: "",
    next_page_results: next !== undefined,
    prev_page_results: false,
    count: rows.length,
    total_results: total,
    results: rows,
  };
}

/** A client whose JSON GET answers `body` and records the requested URLs. */
function fakeClient(body: unknown) {
  const urls: string[] = [];
  const client = {
    getJson: async (path: string, init?: { query?: Record<string, unknown> }) => {
      const url = new URL(path, "http://api.invalid");
      for (const [key, value] of Object.entries(init?.query ?? {})) {
        if (value !== undefined && value !== null) url.searchParams.set(key, String(value));
      }
      urls.push(url.pathname + url.search);
      return body;
    },
    parse: (_schema: unknown, value: unknown) => value,
  };
  return { client: client as never, urls };
}

describe("intake list options (INT-012/INT-031)", () => {
  it("keys every read under the workspace scope", () => {
    const options = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "open" });
    expect(options.queryKey.slice(0, 2)).toEqual(["ws", WS]);
    expect(options.queryKey).toEqual(
      intakeKeys.list(WS, PROJECT, {
        tab: "open",
        filters: { statuses: [-2] },
        orderBy: "-issue__created_at",
      })
    );
  });

  it("includes the tab's status set and the order, and excludes the selection", () => {
    const open = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "open" });
    const closed = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "closed" });
    expect(JSON.stringify(open.queryKey)).toContain("-2");
    expect(JSON.stringify(closed.queryKey)).toContain("1,-1,2");
    expect(JSON.stringify(open.queryKey)).not.toContain("inboxIssueId");
    expect(JSON.stringify(open.queryKey)).toContain("issue__created_at");
  });

  it("never refetches on focus, reconnect or a timer (INT-031)", () => {
    const options = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "open" });
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.refetchIntervalInBackground).toBe(false);
    expect(options.staleTime).toBe(30_000);
  });

  it("honours the enabled flag so a gated project reads nothing (INT-002)", () => {
    const options = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, {
      tab: "open",
      enabled: false,
    });
    expect(options.enabled).toBe(false);
  });

  it("ignores the app default for focus, even when the caller set one", () => {
    const client = new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: true, staleTime: 1 } } });
    const options = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "open" });
    expect(client.getDefaultOptions().queries?.refetchOnWindowFocus).toBe(true);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.staleTime).toBe(30_000);
  });

  it("sends the tab status set, the order, the cursor and the page size", async () => {
    const { client, urls } = fakeClient(page([row("i-1", "Alpha")], 1, undefined));
    const options = intakeListQueryOptions(client, WS, PROJECT, { tab: "closed" });
    const queryFn = options.queryFn as unknown as (ctx: { pageParam: string | undefined }) => Promise<unknown>;
    await queryFn({ pageParam: "cursor-2" });
    const url = new URL(urls[0] ?? "", "http://api.invalid");
    expect(url.pathname).toBe(`/api/workspaces/${WS}/projects/${PROJECT}/intake-issues/`);
    expect(url.searchParams.get("status")).toBe("1,-1,2");
    expect(url.searchParams.get("order_by")).toBe("-issue__created_at");
    expect(url.searchParams.get("cursor")).toBe("cursor-2");
    expect(url.searchParams.get("per_page")).toBe(String(INTAKE_PAGE_SIZE));
  });

  it("follows next_cursor while another page exists and stops when it does not", () => {
    const options = intakeListQueryOptions(fakeClient(page([], 0, undefined)).client, WS, PROJECT, { tab: "open" });
    expect(options.initialPageParam).toBeUndefined();
    const getNext = options.getNextPageParam as (last: IntakeIssueListResponse) => string | undefined;
    expect(getNext(page([], 1, "next-1"))).toBe("next-1");
    expect(getNext(page([], 1, undefined))).toBeUndefined();
    expect(getNext({ ...page([], 1, "next-1"), next_cursor: "" })).toBeUndefined();
  });
});

describe("list projections (INT-004)", () => {
  it("concatenates pages in server order", () => {
    const rows = intakeRows([page([row("i-1", "A"), row("i-2", "B")], 3, "n"), page([row("i-3", "C")], 3, undefined)]);
    expect(rows.map((entry) => entry.issue.name)).toEqual(["A", "B", "C"]);
    expect(intakeRows(undefined)).toEqual([]);
  });

  it("takes the total from the first page only", () => {
    const first = page([row("i-1", "A")], 7, "n");
    const second = { ...first, total_results: 7 };
    expect(intakeTotal([first, second])).toBe(7);
    expect(intakeTotal(undefined)).toBeUndefined();
  });

  it("falls back to the tab default when the filter carries no status set", () => {
    expect(listStatuses("open", undefined)).toEqual([-2]);
    expect(listStatuses("closed", { statuses: [] })).toEqual([]);
    expect(listStatuses("open", { statuses: [0] })).toEqual([0]);
  });
});

describe("prefetchIntakeList", () => {
  it("loads the first page into the cache and tolerates a refusal", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { client } = fakeClient(page([row("i-1", "A")], 1, undefined));
    await prefetchIntakeList(queryClient, client, WS, PROJECT, { tab: "open" });
    const key = intakeKeys.list(WS, PROJECT, {
      tab: "open",
      filters: { statuses: [-2] },
      orderBy: "-issue__created_at",
    });
    expect(queryClient.getQueryData(key)).toBeDefined();
    const broken = { getJson: vi.fn(async () => Promise.reject(new Error("denied"))), parse: vi.fn() };
    await expect(
      prefetchIntakeList(queryClient, broken as never, WS, PROJECT, { tab: "closed" })
    ).resolves.toBeUndefined();
  });
});
