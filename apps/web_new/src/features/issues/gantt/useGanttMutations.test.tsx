// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline writes (parity/specs/issues.md §6, ISS-050–055): batch date
// moves, single-PATCH plants, manual rank PATCHes, optimistic list
// updates, rollback and error notices.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import {
  GANTT_DATES_FAILED_TITLE,
  GANTT_REORDER_FAILED_TITLE,
  useGanttMutations,
  withIssueOrder,
  type GanttMutations,
} from "./useGanttMutations.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function row(id: string, name: string, sortOrder: number): Record<string, unknown> {
  return {
    id,
    name,
    state_id: null,
    sort_order: sortOrder,
    completed_at: null,
    priority: "none",
    start_date: "2026-10-01",
    target_date: "2026-10-03",
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

const ROWS = [row("a", "First", 1000), row("b", "Second", 2000), row("c", "Third", 3000)];

interface Call {
  url: string;
  method: string;
  body: unknown;
}

function stubClient(status = 200) {
  const calls: Call[] = [];
  // Non-CSRF calls park until `release()` so a test can read the
  // optimistic cache before the settle refetch overwrites it.
  let release: (() => void) | null = null;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: JSON.parse(init.body ?? "null") });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    await gate;
    return { status, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls, release: () => release?.() };
}

function setup(status = 200) {
  const stub = stubClient(status);
  const calls = stub.calls;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(issueKeys.lists(SLUG, PROJECT), { results: ROWS, total_count: ROWS.length });
  let handle: GanttMutations | null = null;
  function Probe(): React.ReactElement {
    handle = useGanttMutations(SLUG, PROJECT);
    return <p>{handle.isSaving ? "saving" : "idle"}</p>;
  }
  render(
    <QueryClientProvider client={queryClient}>
      <Probe />
    </QueryClientProvider>
  );
  const data = () =>
    queryClient.getQueryData(issueKeys.lists(SLUG, PROJECT)) as { results: Array<Record<string, unknown>> };
  return {
    calls,
    release: stub.release,
    queryClient,
    data,
    mutations: () => {
      if (handle === null) throw new Error("not mounted");
      return handle;
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("withIssueOrder", () => {
  it("reorders a list envelope by the id sequence", () => {
    const result = withIssueOrder({ results: ROWS, total_count: 3 }, ["c", "a", "b"]) as {
      results: Array<{ id: string }>;
    };
    expect(result.results.map((entry) => entry.id)).toEqual(["c", "a", "b"]);
  });

  it("leaves unknown shapes and unlisted rows in place", () => {
    expect(withIssueOrder(null, ["a"])).toBeNull();
    expect(withIssueOrder([ROWS[0]], ["a"])).toEqual([ROWS[0]]);
  });

  it("keeps a row the new order does not name at its relative place", () => {
    // The list cache may hold a temp create the sequence does not know;
    // it must not be silently dropped or pull ranked rows out of order.
    const temp = { ...ROWS[0], id: "temp-1", name: "Pending" } as (typeof ROWS)[number];
    const result = withIssueOrder({ results: [...ROWS, temp], total_count: 4 }, ["c", "a", "b"]) as {
      results: Array<{ id: string }>;
    };
    expect(result.results.map((entry) => entry.id)).toEqual(["c", "a", "b", "temp-1"]);
  });
});

describe("useGanttMutations dates", () => {
  it("moves a bar through the batch endpoint and paints the cache", async () => {
    const h = setup();
    act(() => h.mutations().moveBar("a", { startDate: "2026-10-04", endDate: "2026-10-06" }));
    await waitFor(() => expect(h.calls.some((call) => call.method === "POST")).toBe(true));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issue-dates/`);
    expect(post?.body).toEqual({
      updates: [{ id: "a", start_date: "2026-10-04", target_date: "2026-10-06" }],
    });
    expect(h.data().results[0]).toMatchObject({ start_date: "2026-10-04", target_date: "2026-10-06" });
    act(() => h.release());
  });

  it("keeps the dropped dates when an older list read resolves late (ISS-051)", async () => {
    const h = setup();
    let resolveRead: ((value: { results: Array<Record<string, unknown>>; total_count: number }) => void) | null = null;
    let readStarted = false;
    const read = h.queryClient.fetchQuery({
      queryKey: issueKeys.lists(SLUG, PROJECT),
      queryFn: () => {
        readStarted = true;
        return new Promise((resolve) => {
          resolveRead = resolve;
        });
      },
      staleTime: 0,
    });
    await waitFor(() => expect(readStarted).toBe(true));

    act(() => h.mutations().moveBar("a", { startDate: "2026-10-04", endDate: "2026-10-06" }));
    await waitFor(() => expect(h.calls.some((call) => call.method === "POST")).toBe(true));
    expect(h.data().results[0]).toMatchObject({ start_date: "2026-10-04", target_date: "2026-10-06" });

    // The request began before the drop and returns the pre-drop dates
    // afterwards. It must not replace the newer optimistic dates.
    resolveRead!({ results: ROWS, total_count: ROWS.length });
    await act(async () => {
      await read.catch(() => undefined);
    });
    expect(h.data().results[0]).toMatchObject({ start_date: "2026-10-04", target_date: "2026-10-06" });
    act(() => h.release());
  });

  it("plants a block through the single issue PATCH", async () => {
    const h = setup();
    act(() => h.mutations().plantBlock("b", { startDate: "2026-10-09", endDate: "2026-10-10" }));
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/b/`);
    expect(patch?.body).toEqual({ start_date: "2026-10-09", target_date: "2026-10-10" });
    expect(h.data().results[1]).toMatchObject({ start_date: "2026-10-09", target_date: "2026-10-10" });
    act(() => h.release());
  });

  it("sends only the ends a resize keeps", async () => {
    const h = setup();
    act(() => h.mutations().resizeBar("c", { startDate: null, endDate: "2026-10-10" }));
    await waitFor(() => expect(h.calls.some((call) => call.method === "POST")).toBe(true));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.body).toEqual({ updates: [{ id: "c", target_date: "2026-10-10" }] });
    act(() => h.release());
  });

  it("sends only the start when a batch write keeps only that end", async () => {
    const h = setup();
    act(() => h.mutations().resizeBar("b", { startDate: "2026-10-02", endDate: null }));
    await waitFor(() => expect(h.calls.some((call) => call.method === "POST")).toBe(true));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.body).toEqual({ updates: [{ id: "b", start_date: "2026-10-02" }] });
    act(() => h.release());
  });

  it("rolls the cache back and notices a refused date write", async () => {
    const h = setup(500);
    act(() => h.mutations().moveBar("a", { startDate: "2026-10-04", endDate: "2026-10-06" }));
    act(() => h.release());
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(GANTT_DATES_FAILED_TITLE, expect.any(String)));
    expect(h.data().results[0]).toMatchObject({ start_date: "2026-10-01", target_date: "2026-10-03" });
  });
});

describe("useGanttMutations reorder", () => {
  it("patches the derived rank and reorders the list optimistically", async () => {
    const h = setup();
    act(() => h.mutations().reorder("c", 1500, ["a", "c", "b"]));
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/c/`);
    expect(patch?.body).toEqual({ sort_order: 1500 });
    expect(h.data().results.map((entry) => entry.id)).toEqual(["a", "c", "b"]);
    act(() => h.release());
  });

  it("rolls the order back and notices a refused reorder", async () => {
    const h = setup(500);
    act(() => h.mutations().reorder("c", 1500, ["a", "c", "b"]));
    act(() => h.release());
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(GANTT_REORDER_FAILED_TITLE, expect.any(String)));
    expect(h.data().results.map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });
});
