// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type IssueListItem, type IssueListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import {
  SHEET_CREATED_TITLE,
  SHEET_UPDATE_FAILED_TITLE,
  sheetIssuesQueryOptions,
  useSheetCreateIssue,
  useSheetCycleUpdate,
  useSheetFieldUpdate,
  useSheetModuleUpdate,
} from "./data.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const OTHER = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function row(id: string, name: string, overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id,
    name,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function page(results: IssueListItem[]): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
  };
}

interface StubState {
  calls: Array<{ url: string; method: string; body: string }>;
  failWrite: boolean;
  release?: (() => void) | null;
  hold?: boolean;
}

function stubClient(state: StubState) {
  return createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    state.calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (state.hold) await new Promise<void>((resolve) => (state.release = resolve));
    if (init.method === undefined || init.method === "GET") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(page([row(ISSUE, "Fix login")])),
      };
    }
    if (init.method === "POST" && url.endsWith("/issues/")) {
      if (state.failWrite) {
        return {
          status: 400,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ detail: "Name is required" }),
        };
      }
      const sent = JSON.parse(init.body ?? "{}") as { name?: string };
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ ...row(OTHER, sent.name ?? "Created"), sequence_id: 9, deleted_at: null }),
      };
    }
    if (state.failWrite) {
      return {
        status: 400,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ detail: "Refused" }),
      };
    }
    if (init.method === "PATCH" || (init.method === "POST" && url.includes("/cycles/"))) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ message: "ok" }) };
    }
    if (init.method === "POST") {
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify({ message: "ok" }) };
    }
    return { status: 204, url, headers: { get: () => null }, text: async () => "" };
  }) as never);
}

function setup(state: StubState = { calls: [], failWrite: false }) {
  const client = stubClient(state);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const key = sheetIssuesQueryOptions(client, SLUG, PROJECT, { orderBy: "-created_at" }).queryKey;
  queryClient.setQueryData(key, {
    pages: [page([row(ISSUE, "Fix login"), row(OTHER, "Second")])],
    pageParams: [""],
  });
  queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), row(ISSUE, "Fix login"));
  queryClient.setQueryData(issueKeys.list(SLUG, PROJECT, {} as never), {
    results: [row(ISSUE, "Fix login")],
  });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
    </QueryClientProvider>
  );
  return { queryClient, key, wrapper };
}

function firstRow(queryClient: QueryClient, key: readonly unknown[]): IssueListItem {
  const data = queryClient.getQueryData<{ pages: IssueListResponse[] }>(key);
  return data!.pages[0]!.results[0]!;
}

beforeEach(() => {
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("spreadsheet writes", () => {
  it("patches the cell optimistically and keeps the server patch", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetFieldUpdate(SLUG, PROJECT), { wrapper });
    const pending = result.current.mutateAsync({ issueId: ISSUE, patch: { priority: "urgent" } });
    await waitFor(() => expect(firstRow(queryClient, key).priority).toBe("urgent"));
    await act(async () => pending);
    expect(state.calls.some((call) => call.method === "PATCH" && call.url.includes(`/issues/${ISSUE}/`))).toBe(true);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls back and toasts when a cell write is refused", async () => {
    const state: StubState = { calls: [], failWrite: true };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetFieldUpdate(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ issueId: ISSUE, patch: { priority: "urgent" } })
      ).rejects.toBeInstanceOf(ApiError);
    });
    expect(firstRow(queryClient, key).priority).toBe("none");
    expect(notifyError).toHaveBeenCalledWith(SHEET_UPDATE_FAILED_TITLE, "Refused");
  });

  it("sends the sub_issue filter with every sheet list read", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const client = stubClient(state);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = sheetIssuesQueryOptions(client, SLUG, PROJECT, { orderBy: "-created_at", showSubIssues: true });
    await queryClient.fetchInfiniteQuery(options);
    const call = state.calls.find((entry) => entry.url.includes(`/projects/${PROJECT}/issues/`));
    expect(new URL(call?.url ?? "", "https://api.test").searchParams.get("sub_issue")).toBe("true");
    const off = sheetIssuesQueryOptions(client, SLUG, PROJECT, { orderBy: "-created_at", showSubIssues: false });
    expect(off.queryKey).not.toEqual(options.queryKey);
  });

  it("sends the stored rich filter and keys the sheet query on the expression", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const client = stubClient(state);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const expression = { priority__in: "urgent" };
    const options = sheetIssuesQueryOptions(client, SLUG, PROJECT, {
      orderBy: "-created_at",
      richExpression: expression,
    });
    await queryClient.fetchInfiniteQuery(options);
    const call = state.calls.find((entry) => entry.url.includes(`/projects/${PROJECT}/issues/`));
    expect(new URL(call?.url ?? "", "https://api.test").searchParams.get("filters")).toBe(JSON.stringify(expression));
    const other = sheetIssuesQueryOptions(client, SLUG, PROJECT, {
      orderBy: "-created_at",
      richExpression: { priority__in: "low" },
    });
    expect(other.queryKey).not.toEqual(options.queryKey);
    const empty = sheetIssuesQueryOptions(client, SLUG, PROJECT, { orderBy: "-created_at" });
    expect(empty.queryKey).not.toEqual(options.queryKey);
  });

  it("adds to and clears from a cycle through the bridge", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetCycleUpdate(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ issueId: ISSUE, cycleId: CYCLE, previousCycleId: null });
    });
    expect(firstRow(queryClient, key).cycle_id).toBe(CYCLE);
    expect(
      state.calls.some((call) => call.method === "POST" && call.url.includes(`/cycles/${CYCLE}/cycle-issues/`))
    ).toBe(true);
    await act(async () => {
      await result.current.mutateAsync({ issueId: ISSUE, cycleId: null, previousCycleId: CYCLE });
    });
    expect(firstRow(queryClient, key).cycle_id).toBeNull();
    expect(state.calls.some((call) => call.method === "DELETE" && call.url.includes(`/cycles/${CYCLE}/`))).toBe(true);
  });

  it("clears a cycle without a previous cycle", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetCycleUpdate(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ issueId: ISSUE, cycleId: null, previousCycleId: null });
    });
    expect(firstRow(queryClient, key).cycle_id).toBeNull();
    expect(state.calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
  });

  it("applies only the module diff", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetModuleUpdate(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ issueId: ISSUE, moduleIds: [MODULE], previousModuleIds: [] });
    });
    expect(firstRow(queryClient, key).module_ids).toEqual([MODULE]);
    const call = state.calls.find((entry) => entry.method === "POST" && entry.url.includes("modules"));
    expect(call?.body).toContain(MODULE);
  });

  it("sends removed modules and patches detail and flat-list caches", async () => {
    const state: StubState = { calls: [], failWrite: false };
    const { queryClient, key, wrapper } = setup(state);
    queryClient.setQueryData(key, {
      pages: [page([row(ISSUE, "Fix login", { module_ids: [MODULE] })])],
      pageParams: [""],
    });
    const { result } = renderHook(() => useSheetModuleUpdate(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ issueId: ISSUE, moduleIds: [], previousModuleIds: [MODULE] });
    });
    const call = state.calls.find((entry) => entry.method === "POST" && entry.url.includes("modules"));
    expect(call?.body).toContain("removed_modules");
    expect(queryClient.getQueryData<IssueListItem>(issueKeys.detail(SLUG, ISSUE))?.module_ids).toEqual([]);
    expect(
      queryClient.getQueryData<{ results: IssueListItem[] }>(issueKeys.list(SLUG, PROJECT, {} as never))?.results[0]
        ?.module_ids
    ).toEqual([]);
  });

  it("quick-add inserts a temp row, then swaps in the created row and toasts", async () => {
    const state: StubState = { calls: [], failWrite: false, hold: true };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetCreateIssue(SLUG, PROJECT), { wrapper });
    let pending: Promise<unknown> = Promise.resolve();
    act(() => {
      pending = result.current.mutateAsync({ name: "New sheet item" });
    });
    await waitFor(() => expect(firstRow(queryClient, key).id.startsWith("temp-")).toBe(true));
    state.release?.();
    await act(async () => {
      await pending;
    });
    expect(firstRow(queryClient, key)).toMatchObject({ id: OTHER, name: "New sheet item" });
    expect(notifySuccess).toHaveBeenCalledWith(SHEET_CREATED_TITLE, expect.any(String));
  });

  it("quick-add removes the temp row and reports a failure", async () => {
    const state: StubState = { calls: [], failWrite: true };
    const { queryClient, key, wrapper } = setup(state);
    const { result } = renderHook(() => useSheetCreateIssue(SLUG, PROJECT), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ name: "Nope" })).rejects.toBeInstanceOf(ApiError);
    });
    await waitFor(() => expect(firstRow(queryClient, key).id).toBe(ISSUE));
    expect(notifyError).toHaveBeenCalledWith(SHEET_UPDATE_FAILED_TITLE, "Name is required");
  });
});
