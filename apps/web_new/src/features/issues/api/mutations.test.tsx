// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type Issue, type IssueListItem, type IssueListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { updateIssue: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import type { LiteResolution } from "./browse.js";
import { issueKeys } from "./keys.js";
import {
  applyIssuePatchDetail,
  applyIssuePatchList,
  applyIssuePatchLite,
  ISSUE_SAVE_FALLBACK,
  saveFailureDetail,
  useUpdateIssue,
} from "./mutations.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const OTHER = "323e4567-e89b-12d3-a456-426614174002";

function issueRow(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
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
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>old</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function liteResolution(overrides: Partial<LiteResolution["lite"]> = {}): LiteResolution {
  return {
    lite: {
      id: ISSUE,
      sequence_id: 42,
      name: "Fix login",
      description_html: "<p>old</p>",
      sort_order: 1,
      project_id: PROJECT,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
      created_by: null,
      updated_by: null,
      is_draft: false,
      is_epic: false,
      is_intake: false,
      is_synced: false,
      archived_at: null,
      ...overrides,
    },
  };
}

function listRow(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
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

function listPage(): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 1,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 1,
    total_results: 1,
    results: [listRow()],
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("saveFailureDetail", () => {
  it("prefers the server message, falling back when empty", () => {
    expect(saveFailureDetail(new ApiError({ status: 403, code: "http", message: "Down" }))).toBe("Down");
    expect(saveFailureDetail(new ApiError({ status: 403, code: "http", message: "" }))).toBe(ISSUE_SAVE_FALLBACK);
    expect(saveFailureDetail(new Error("boom"))).toBe(ISSUE_SAVE_FALLBACK);
  });
});

describe("issue patch updaters", () => {
  it("paints the detail cache, leaving absent caches alone", () => {
    expect(applyIssuePatchDetail(issueRow(), { name: "New" })?.name).toBe("New");
    expect(applyIssuePatchDetail(issueRow(), { description_html: "<p>new</p>" })?.description_html).toBe("<p>new</p>");
    expect(applyIssuePatchDetail(issueRow(), { priority: "low" })?.priority).toBe("low");
    expect(applyIssuePatchDetail(issueRow(), { state_id: "state-b" })?.state_id).toBe("state-b");
    expect(applyIssuePatchDetail(issueRow(), { assignee_ids: [] })?.assignee_ids).toEqual([]);
    expect(applyIssuePatchDetail(issueRow(), {})).toEqual(issueRow());
    expect(applyIssuePatchDetail(undefined, { name: "New" })).toBeUndefined();
  });

  it("paints the lite row only when it names this item", () => {
    expect(applyIssuePatchLite(liteResolution(), ISSUE, { name: "New" })?.lite?.name).toBe("New");
    expect(applyIssuePatchLite(liteResolution(), OTHER, { name: "New" })).toEqual(liteResolution());
    expect(applyIssuePatchLite({ lite: null }, ISSUE, { name: "New" })).toEqual({ lite: null });
    expect(applyIssuePatchLite(undefined, ISSUE, { name: "New" })).toBeUndefined();
  });

  it("paints every writable row field, so a property edit reads back immediately", () => {
    const painted = applyIssuePatchDetail(issueRow(), {
      state_id: "623e4567-e89b-12d3-a456-426614174005",
      priority: "low",
      assignee_ids: ["323e4567-e89b-12d3-a456-426614174002"],
      label_ids: ["423e4567-e89b-12d3-a456-426614174003"],
      estimate_point: null,
      start_date: "2026-01-01",
      target_date: null,
      parent_id: null,
      sort_order: 7,
    });
    expect(painted).toMatchObject({
      state_id: "623e4567-e89b-12d3-a456-426614174005",
      priority: "low",
      assignee_ids: ["323e4567-e89b-12d3-a456-426614174002"],
      label_ids: ["423e4567-e89b-12d3-a456-426614174003"],
      estimate_point: null,
      start_date: "2026-01-01",
      target_date: null,
      parent_id: null,
      sort_order: 7,
    });
  });

  it("leaves write-only and execution-target fields out of the detail paint", () => {
    const painted = applyIssuePatchDetail(issueRow({ assigned_pod_id: null, agent_executor: "local_runner" }), {
      git_work_branch: "pi-dash/wip",
      assigned_pod_id: "923e4567-e89b-12d3-a456-426614174009",
      agent_executor: "cloud_agent",
    });
    expect(painted?.agent_executor).toBe("local_runner");
    expect(painted?.assigned_pod_id).toBeNull();
    expect(painted).not.toHaveProperty("git_work_branch");
  });

  it("paints property fields on the matching list row", () => {
    const painted = applyIssuePatchList(listPage(), ISSUE, {
      state_id: "623e4567-e89b-12d3-a456-426614174005",
      priority: "urgent",
      assignee_ids: ["323e4567-e89b-12d3-a456-426614174002"],
    });
    expect(painted?.results[0]).toMatchObject({
      state_id: "623e4567-e89b-12d3-a456-426614174005",
      priority: "urgent",
      assignee_ids: ["323e4567-e89b-12d3-a456-426614174002"],
    });
    expect(applyIssuePatchList(listPage(), OTHER, { priority: "urgent" })).toEqual(listPage());
  });

  it("renames the matching list row only", () => {
    const renamed = applyIssuePatchList(listPage(), ISSUE, { name: "New" });
    expect(renamed?.results[0]?.name).toBe("New");
    expect(applyIssuePatchList(listPage(), ISSUE, { priority: "low" })?.results[0]?.priority).toBe("low");
    expect(applyIssuePatchList(listPage(), OTHER, { name: "New" })).toEqual(listPage());
    expect(applyIssuePatchList(listPage(), ISSUE, { description_html: "<p>x</p>" })).toEqual(listPage());
    expect(applyIssuePatchList(undefined, ISSUE, { name: "New" })).toBeUndefined();
  });
});

describe("useUpdateIssue", () => {
  function seed(queryClient: QueryClient): void {
    queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), issueRow());
    queryClient.setQueryData(issueKeys.browse(SLUG, "ACME-42"), liteResolution());
    queryClient.setQueryData(issueKeys.list(SLUG, PROJECT, { layout: "list" }), listPage());
  }

  it("paints every cached copy, then revalidates on success", async () => {
    const queryClient = freshClient();
    seed(queryClient);
    contractMocks.updateIssue.mockResolvedValue(undefined);
    const { result } = renderHook(
      () => useUpdateIssue({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE, failureTitle: "Nope" }),
      { wrapper: wrapper(queryClient) }
    );

    act(() => {
      result.current.mutate({ name: "New" });
    });

    // Optimistic paint lands before the write settles (onMutate yields
    // on the in-flight cancel first).
    await waitFor(() => expect(queryClient.getQueryData<Issue>(issueKeys.detail(SLUG, ISSUE))?.name).toBe("New"));
    expect(queryClient.getQueryData<LiteResolution>(issueKeys.browse(SLUG, "ACME-42"))?.lite?.name).toBe("New");
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.updateIssue).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, ISSUE, { name: "New" });
    expect(queryClient.getQueryState(issueKeys.detail(SLUG, ISSUE))?.isInvalidated).toBe(true);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls every cached copy back and notices on failure", async () => {
    const queryClient = freshClient();
    seed(queryClient);
    contractMocks.updateIssue.mockRejectedValue(new ApiError({ status: 403, code: "http", message: "Down" }));
    const { result } = renderHook(
      () => useUpdateIssue({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE, failureTitle: "Nope" }),
      { wrapper: wrapper(queryClient) }
    );

    act(() => {
      result.current.mutate({ name: "New" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(queryClient.getQueryData<Issue>(issueKeys.detail(SLUG, ISSUE))?.name).toBe("Fix login");
    expect(queryClient.getQueryData<LiteResolution>(issueKeys.browse(SLUG, "ACME-42"))?.lite?.name).toBe("Fix login");
    expect(
      queryClient.getQueryData<IssueListResponse>(issueKeys.list(SLUG, PROJECT, { layout: "list" }))?.results[0]?.name
    ).toBe("Fix login");
    expect(notifyError).toHaveBeenCalledWith("Nope", "Down");
  });

  it("falls back to the generic detail when the failure is mute", async () => {
    const queryClient = freshClient();
    seed(queryClient);
    contractMocks.updateIssue.mockRejectedValue(new Error("boom"));
    const { result } = renderHook(
      () => useUpdateIssue({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE, failureTitle: "Nope" }),
      { wrapper: wrapper(queryClient) }
    );

    act(() => {
      result.current.mutate({ name: "New" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Nope", ISSUE_SAVE_FALLBACK);
  });

  it("revalidates the version history only for description writes", async () => {
    const queryClient = freshClient();
    seed(queryClient);
    contractMocks.updateIssue.mockResolvedValue(undefined);
    const versionsKey = issueKeys.descriptionVersions(SLUG, PROJECT, ISSUE);
    queryClient.setQueryData(versionsKey, { results: [] });
    const { result } = renderHook(
      () => useUpdateIssue({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE, failureTitle: "Nope" }),
      { wrapper: wrapper(queryClient) }
    );

    act(() => {
      result.current.mutate({ name: "New" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryState(versionsKey)?.isInvalidated).toBe(false);

    act(() => {
      result.current.mutate({ description_html: "<p>new</p>" });
    });
    await waitFor(() => expect(queryClient.getQueryState(versionsKey)?.isInvalidated).toBe(true));
  });
});
