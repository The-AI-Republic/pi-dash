// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, type QueryFunctionContext } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { createClient } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { listPageQueryOptions, sectionCursor, useListPages } from "./pages.js";
import type { IssueGroupSection } from "./grouping.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";

function uuidFrom(value: string): string {
  let hash = 0;
  for (const char of value) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return `00000000-0000-4000-8000-${hash.toString(16).padStart(12, "0").slice(-12)}`;
}

function row(id: string, sequence: number): IssueListItem {
  return {
    id: uuidFrom(id),
    name: `Issue ${sequence}`,
    state_id: null,
    sort_order: sequence,
    completed_at: null,
    priority: "none",
    sequence_id: sequence,
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
  };
}

function ids(rows: IssueListItem[]): string[] {
  return rows.map((entry) => entry.id);
}

function envelope(rows: IssueListItem[], next?: string): Record<string, unknown> {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows.length,
    next_cursor: next ?? "",
    prev_cursor: "",
    next_page_results: next !== undefined,
    prev_page_results: false,
    count: rows.length,
    total_results: rows.length,
    results: rows,
  };
}

function groupedEnvelope(buckets: Record<string, { rows: IssueListItem[]; total: number }>): Record<string, unknown> {
  const results: Record<string, unknown> = {};
  for (const [key, bucket] of Object.entries(buckets)) {
    results[key] = { results: bucket.rows, total_results: bucket.total };
  }
  return {
    grouped_by: "state_id",
    sub_grouped_by: null,
    total_count: Object.values(buckets).reduce((sum, bucket) => sum + bucket.total, 0),
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: Object.values(buckets).reduce((sum, bucket) => sum + bucket.rows.length, 0),
    total_results: Object.values(buckets).reduce((sum, bucket) => sum + bucket.total, 0),
    results,
  };
}

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

const context = {
  client: {},
  queryKey: [],
  meta: undefined,
  signal: new AbortController().signal,
} as unknown as QueryFunctionContext;

afterEach(() => resetAppClient());

describe("listPageQueryOptions", () => {
  it.each([
    ["project", { scope: "project", projectId: PROJECT } as const, "/issues/"],
    ["cycle", { scope: "cycle", projectId: PROJECT, cycleId: CYCLE } as const, `/cycles/${CYCLE}/cycle-issues/`],
    ["module", { scope: "module", projectId: PROJECT, moduleId: MODULE } as const, `/modules/${MODULE}/issues/`],
  ])("reads the flat %s list at its own path", async (_label, entity, suffix) => {
    const { client, calls } = recordingClient(envelope([row("one", 1)]));
    const options = listPageQueryOptions(client, "acme", { entity, groupBy: null, orderBy: "sort_order" });
    await options.queryFn!(context as never);
    const url = new URL(calls[0]!.url);
    expect(url.pathname).toBe(`/api/workspaces/acme/projects/${PROJECT}${suffix}`);
    expect(url.searchParams.get("per_page")).toBe("100");
    expect(url.searchParams.get("group_by")).toBeNull();
  });

  it.each([
    ["project", { scope: "project", projectId: PROJECT } as const, "/issues/"],
    ["cycle", { scope: "cycle", projectId: PROJECT, cycleId: CYCLE } as const, `/cycles/${CYCLE}/cycle-issues/`],
    ["module", { scope: "module", projectId: PROJECT, moduleId: MODULE } as const, `/modules/${MODULE}/issues/`],
  ])("reads the grouped %s list with the server key and cursor", async (_label, entity, suffix) => {
    const { client, calls } = recordingClient(groupedEnvelope({ todo: { rows: [row("one", 1)], total: 1 } }));
    const options = listPageQueryOptions(client, "acme", {
      entity,
      groupBy: "state",
      orderBy: "-created_at",
      cursor: "50:0:0",
    });
    await options.queryFn!(context as never);
    const url = new URL(calls[0]!.url);
    expect(url.pathname).toBe(`/api/workspaces/acme/projects/${PROJECT}${suffix}`);
    expect(url.searchParams.get("per_page")).toBe("50");
    expect(url.searchParams.get("group_by")).toBe("state_id");
    expect(url.searchParams.get("order_by")).toBe("-created_at");
    expect(url.searchParams.get("cursor")).toBe("50:0:0");
  });

  it("rejects entities without a project scope", async () => {
    const { client } = recordingClient(envelope([]));
    const options = listPageQueryOptions(client, "acme", {
      entity: { scope: "workspace" },
      groupBy: null,
      orderBy: "sort_order",
    });
    await expect(options.queryFn!(context as never)).rejects.toThrow("project-scoped");
  });

  it("builds the cursor from the section row count", () => {
    expect(sectionCursor(0)).toBe("50:0:0");
    expect(sectionCursor(49)).toBe("50:0:0");
    expect(sectionCursor(50)).toBe("50:1:0");
    expect(sectionCursor(140, 100)).toBe("100:1:0");
  });
});

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

function hookClient(handler: (url: URL) => Record<string, unknown>): QueryClient {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(handler(new URL(url))),
  })) as never);
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

describe("useListPages", () => {
  it("appends flat cursor pages and reports the next page", async () => {
    const client = hookClient((url) =>
      url.searchParams.get("cursor") === "100:1:0" ? envelope([row("two", 2)]) : envelope([row("one", 1)], "100:1:0")
    );
    const { result } = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, null, "sort_order", new Map(), false),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(result.current.isPending).toBe(false));
    expect(ids(result.current.sections[0]?.rows ?? [])).toEqual([uuidFrom("one")]);
    expect(result.current.hasMore).toBe(true);
    act(() => result.current.loadMore());
    await waitFor(() =>
      expect(ids(result.current.sections[0]?.rows ?? [])).toEqual([uuidFrom("one"), uuidFrom("two")])
    );
    expect(result.current.hasMore).toBe(false);
  });

  it("ignores loadMore while grouped or when no cursor remains", async () => {
    const client = hookClient(() => envelope([row("one", 1)]));
    const { result } = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, null, "sort_order", new Map(), false),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(result.current.isPending).toBe(false));
    act(() => result.current.loadMore());
    expect(result.current.sections[0]?.rows).toHaveLength(1);
  });

  it("pages each grouped section independently", async () => {
    const client = hookClient((url) => {
      const cursor = url.searchParams.get("cursor");
      if (cursor === "50:0:0") {
        return groupedEnvelope({ todo: { rows: [row("todo-2", 2)], total: 51 } });
      }
      return groupedEnvelope({
        todo: { rows: [row("todo-1", 1)], total: 51 },
        done: { rows: [row("done-1", 3)], total: 1 },
      });
    });
    const { result } = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, "state", "sort_order", new Map(), true),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(result.current.isPending).toBe(false));
    expect(result.current.sections.map((section) => section.key)).toEqual(["todo", "done"]);
    const todo = result.current.sections.find((section) => section.key === "todo") as IssueGroupSection;
    expect(result.current.sectionMore(todo)).toBe(true);
    expect(result.current.total).toBe(52);
    act(() => result.current.loadMoreSection(todo));
    await waitFor(() =>
      expect(ids(result.current.sections.find((section) => section.key === "todo")?.rows ?? [])).toEqual([
        uuidFrom("todo-1"),
        uuidFrom("todo-2"),
      ])
    );
    expect(result.current.sections.find((section) => section.key === "done")?.rows).toHaveLength(1);
    expect(result.current.hasMore).toBe(false);
  });

  it("drops duplicate section cursors", async () => {
    const client = hookClient(() => groupedEnvelope({ todo: { rows: [row("todo-1", 1)], total: 51 } }));
    const { result } = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, "state", "sort_order", new Map(), true),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(result.current.isPending).toBe(false));
    const todo = result.current.sections[0] as IssueGroupSection;
    act(() => result.current.loadMoreSection(todo));
    act(() => result.current.loadMoreSection(todo));
    expect(result.current.sections[0]?.rows).toHaveLength(1);
  });

  it("keeps empty groups only when the display flag asks", async () => {
    const client = hookClient(() =>
      groupedEnvelope({ todo: { rows: [row("todo-1", 1)], total: 1 }, empty: { rows: [], total: 0 } })
    );
    const hidden = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, "state", "sort_order", new Map(), false),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(hidden.result.current.isPending).toBe(false));
    expect(hidden.result.current.sections.map((section) => section.key)).toEqual(["todo"]);
    hidden.unmount();
    const shown = renderHook(
      () => useListPages("acme", { scope: "project", projectId: PROJECT }, "state", "sort_order", new Map(), true),
      { wrapper: wrapper(client) }
    );
    await waitFor(() => expect(shown.result.current.isPending).toBe(false));
    expect(shown.result.current.sections.map((section) => section.key)).toEqual(["todo", "empty"]);
  });
});
