// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue writes (parity/specs/issues.md §15, ISS-175–177): attach
// posts the chosen ids and paints the first page, remove patches the
// parent to null, delete destroys, and each inline edit sends exactly
// one field. A refused write restores the cache and toasts the failure.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { IssuePatch, SubIssueItem, SubIssuesResponse, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  SUB_ISSUES_ADDED_TITLE,
  SUB_ISSUES_ADD_FAILED_TITLE,
  SUB_ISSUE_DELETED_TITLE,
  SUB_ISSUE_DELETE_FAILED_TITLE,
  SUB_ISSUE_REMOVED_TITLE,
  SUB_ISSUE_REMOVE_FAILED_TITLE,
  SUB_ISSUE_UPDATE_FAILED_TITLE,
  attachedRows,
  useAttachSubIssues,
  useDeleteSubIssue,
  usePatchSubIssue,
  useRemoveSubIssue,
  withRowPatched,
  withRowRemoved,
  withRowsAttached,
} from "./mutations.js";
import { subIssuesKey } from "./queries.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";
const ADDED = "523e4567-e89b-12d3-a456-426614174004";
const STATE = "623e4567-e89b-12d3-a456-426614174005";

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: CHILD,
    name: "Child one",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: "2026-10-01",
    target_date: "2026-10-10",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
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
    state_group: "started",
    ...overrides,
  };
}

function page(overrides: Partial<SubIssuesResponse> = {}): SubIssuesResponse {
  return {
    sub_issues: [row()],
    state_distribution: { started: [CHILD] },
    total_count: 1,
    ...overrides,
  };
}

function ok(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

interface StubOptions {
  fail?: boolean;
  addRows?: unknown[];
  epic?: boolean;
}

function stubApi(options: StubOptions = {}) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return ok(url, 200, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if (options.fail === true) return ok(url, 500, { detail: "server refused" });
    if (
      method === "POST" &&
      (url.endsWith(`/issues/${PARENT}/sub-issues/`) || url.endsWith(`/epics/${PARENT}/issues/`))
    ) {
      return ok(url, 201, {
        sub_issues: options.addRows ?? [
          {
            id: ADDED,
            name: "Second child",
            state_id: null,
            sort_order: 2,
            completed_at: null,
            estimate_point: null,
            priority: "none",
            complexity_score: 0,
            start_date: null,
            target_date: null,
            sequence_id: 8,
            project_id: PROJECT,
            parent_id: PARENT,
            assigned_pod_id: null,
            agent_executor: null,
            created_at: "2026-10-02T00:00:00Z",
            updated_at: "2026-10-02T00:00:00Z",
            created_by: null,
            updated_by: null,
            is_draft: false,
            archived_at: null,
            is_synced: false,
          },
        ],
        state_distribution: { started: [CHILD], backlog: [ADDED] },
      });
    }
    return ok(url, 204, {});
  }) as never);
  return calls;
}

function wrapper(client: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

function seeded(overrides: Partial<SubIssuesResponse> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(subIssuesKey(WS, PROJECT, PARENT), {
    pages: [page(overrides)],
    pageParams: [{ cursor: undefined }],
  });
  return client;
}

function cachedPages(client: QueryClient): SubIssuesResponse[] {
  const data = client.getQueryData<{ pages: SubIssuesResponse[] }>(subIssuesKey(WS, PROJECT, PARENT));
  return data?.pages ?? [];
}

const SCOPE = { workspaceSlug: WS, projectId: PROJECT, parentId: PARENT, isEpic: false };

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("page transforms", () => {
  it("drops a row from the page, the distribution and the total", () => {
    const next = withRowRemoved(page({ state_distribution: { started: [CHILD, ADDED] }, total_count: 2 }), CHILD);
    expect(next.sub_issues).toEqual([]);
    expect(next.state_distribution).toEqual({ started: [ADDED] });
    expect(next.total_count).toBe(1);
  });

  it("removes a row from a page without a total", () => {
    const next = withRowRemoved(page({ total_count: undefined }), CHILD);
    expect(next.total_count).toBeUndefined();
  });

  it("patches one row and leaves the others alone", () => {
    const both = page({ sub_issues: [row(), row({ id: ADDED })] });
    const next = withRowPatched(both, CHILD, { name: "Renamed" });
    expect(next.sub_issues.map((entry) => entry.name)).toEqual(["Renamed", "Child one"]);
  });

  it("appends only rows the page does not already hold", () => {
    const next = withRowsAttached(page({ total_count: 1 }), [row(), row({ id: ADDED })], { backlog: [ADDED] });
    expect(next.sub_issues.map((entry) => entry.id)).toEqual([CHILD, ADDED]);
    expect(next.total_count).toBe(2);
    expect(next.state_distribution).toEqual({ backlog: [ADDED] });
  });

  it("keeps an absent total absent and ignores duplicate answers", () => {
    const next = withRowsAttached(page({ total_count: undefined }), [row()], {});
    expect(next.total_count).toBeUndefined();
    expect(next.sub_issues).toHaveLength(1);
  });

  it("shapes added rows as list rows with the distribution's state group", () => {
    const [shaped] = attachedRows(
      [
        {
          id: ADDED,
          name: "Second child",
          state_id: STATE,
          sort_order: 2,
          completed_at: null,
          estimate_point: null,
          priority: "none",
          complexity_score: 0,
          start_date: null,
          target_date: null,
          sequence_id: 8,
          project_id: PROJECT,
          parent_id: PARENT,
          assigned_pod_id: null,
          agent_executor: null,
          created_at: "2026-10-02T00:00:00Z",
          updated_at: "2026-10-02T00:00:00Z",
          created_by: null,
          updated_by: null,
          is_draft: false,
          archived_at: null,
          is_synced: false,
        },
      ],
      { started: [ADDED] }
    );
    expect(shaped).toMatchObject({
      id: ADDED,
      state_group: "started",
      parent_id: PARENT,
      cycle_id: null,
      sub_issues_count: null,
      attachment_count: null,
      link_count: null,
      assignee_ids: [],
      module_ids: [],
      label_ids: [],
      is_draft: false,
    });
  });

  it("leaves the state group null when the distribution does not name the row", () => {
    const [shaped] = attachedRows(
      [
        {
          id: ADDED,
          name: "Second child",
          state_id: null,
          sort_order: 2,
          completed_at: null,
          estimate_point: null,
          priority: "none",
          complexity_score: 0,
          start_date: null,
          target_date: null,
          sequence_id: 8,
          project_id: PROJECT,
          parent_id: PARENT,
          assigned_pod_id: null,
          agent_executor: null,
          created_at: "2026-10-02T00:00:00Z",
          updated_at: "2026-10-02T00:00:00Z",
          created_by: null,
          updated_by: null,
          is_draft: false,
          archived_at: null,
          is_synced: false,
        },
      ],
      {}
    );
    expect(shaped?.state_group).toBeNull();
  });
});

describe("useAttachSubIssues (ISS-175)", () => {
  it("posts the ids, appends the answer and toasts", async () => {
    const calls = stubApi();
    const client = seeded();
    const { result } = renderHook(() => useAttachSubIssues(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate([ADDED]);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({ sub_issue_ids: [ADDED] });
    expect(cachedPages(client)[0]?.sub_issues.map((entry) => entry.id)).toEqual([CHILD, ADDED]);
    expect(cachedPages(client)[0]?.total_count).toBe(2);
    expect(notifySuccess).toHaveBeenCalledWith(SUB_ISSUES_ADDED_TITLE, "1 item attached.");
  });

  it("pluralizes the notice for several ids", async () => {
    stubApi();
    const client = seeded();
    const { result } = renderHook(() => useAttachSubIssues(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate([ADDED, "723e4567-e89b-12d3-a456-426614174006"]);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(SUB_ISSUES_ADDED_TITLE, "2 items attached."));
  });

  it("leaves a cache with no pages untouched on success", async () => {
    stubApi();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(subIssuesKey(WS, PROJECT, PARENT), { pages: [], pageParams: [] });
    const { result } = renderHook(() => useAttachSubIssues(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate([ADDED]);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.getQueryData(subIssuesKey(WS, PROJECT, PARENT))).toEqual({ pages: [], pageParams: [] });
  });

  it("leaves an open cache in a shape it cannot read untouched", async () => {
    stubApi();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(subIssuesKey(WS, PROJECT, PARENT), { notPages: true });
    const { result } = renderHook(() => useAttachSubIssues(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate([ADDED]);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.getQueryData(subIssuesKey(WS, PROJECT, PARENT))).toEqual({ notPages: true });
  });

  it("attaches an epic's children through the epics collection", async () => {
    const calls = stubApi();
    const client = seeded();
    const { result } = renderHook(() => useAttachSubIssues({ ...SCOPE, isEpic: true }), { wrapper: wrapper(client) });
    result.current.mutate([ADDED]);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/projects/${PROJECT}/epics/${PARENT}/issues/`);
    expect(post?.body).toEqual({ sub_issue_ids: [ADDED] });
    expect(notifySuccess).toHaveBeenCalledWith(SUB_ISSUES_ADDED_TITLE, "1 item attached.");
  });

  it("toasts the server detail when the attach is refused", async () => {
    stubApi({ fail: true });
    const client = seeded();
    const { result } = renderHook(() => useAttachSubIssues(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate([ADDED]);
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(SUB_ISSUES_ADD_FAILED_TITLE, "server refused");
    expect(cachedPages(client)[0]?.sub_issues).toHaveLength(1);
  });
});

describe("useRemoveSubIssue (ISS-176)", () => {
  it("unlinks by patching the child parent to null and drops the row", async () => {
    const calls = stubApi();
    const client = seeded();
    const { result } = renderHook(() => useRemoveSubIssue(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate(row());
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/projects/${PROJECT}/issues/${CHILD}/`);
    expect(patch?.body).toEqual({ parent_id: null });
    expect(cachedPages(client)[0]?.sub_issues).toEqual([]);
    expect(notifySuccess).toHaveBeenCalledWith(SUB_ISSUE_REMOVED_TITLE, "Child one is no longer a sub-work item.");
  });

  it("restores the page and toasts when the detach is refused", async () => {
    stubApi({ fail: true });
    const client = seeded();
    const { result } = renderHook(() => useRemoveSubIssue(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate(row());
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(cachedPages(client)[0]?.sub_issues.map((entry) => entry.id)).toEqual([CHILD]);
    expect(notifyError).toHaveBeenCalledWith(SUB_ISSUE_REMOVE_FAILED_TITLE, "server refused");
  });
});

describe("useDeleteSubIssue (ISS-176)", () => {
  it("deletes the child and drops the row", async () => {
    const calls = stubApi();
    const client = seeded();
    const { result } = renderHook(() => useDeleteSubIssue(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate(row());
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.find((call) => call.method === "DELETE")?.url).toContain(`/issues/${CHILD}/`);
    expect(cachedPages(client)[0]?.sub_issues).toEqual([]);
    expect(notifySuccess).toHaveBeenCalledWith(SUB_ISSUE_DELETED_TITLE, "Child one was deleted.");
  });

  it("restores the page and toasts when the delete is refused", async () => {
    stubApi({ fail: true });
    const client = seeded();
    const { result } = renderHook(() => useDeleteSubIssue(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate(row());
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(cachedPages(client)[0]?.sub_issues).toHaveLength(1);
    expect(notifyError).toHaveBeenCalledWith(SUB_ISSUE_DELETE_FAILED_TITLE, "server refused");
  });
});

describe("usePatchSubIssue (ISS-177)", () => {
  it("patches exactly the one field it is handed", async () => {
    const calls = stubApi();
    const client = seeded();
    const { result } = renderHook(() => usePatchSubIssue(SCOPE), { wrapper: wrapper(client) });
    const patch: IssuePatch = { priority: "low" };
    result.current.mutate({ row: row(), patch });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ priority: "low" });
    expect(cachedPages(client)[0]?.sub_issues[0]?.priority).toBe("low");
  });

  it("restores the row and toasts when the edit is refused", async () => {
    stubApi({ fail: true });
    const client = seeded();
    const { result } = renderHook(() => usePatchSubIssue(SCOPE), { wrapper: wrapper(client) });
    result.current.mutate({ row: row(), patch: { name: "Renamed" } });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(cachedPages(client)[0]?.sub_issues[0]?.name).toBe("Child one");
    expect(notifyError).toHaveBeenCalledWith(SUB_ISSUE_UPDATE_FAILED_TITLE, "server refused");
  });
});
