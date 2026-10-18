// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateIssue: vi.fn(),
  addCycle: vi.fn(),
  removeCycle: vi.fn(),
  setModules: vi.fn(),
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  updateIssue: mocks.updateIssue,
  addIssueToCycle: mocks.addCycle,
  removeIssueFromCycle: mocks.removeCycle,
  setIssueModules: mocks.setModules,
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: mocks.notifyError,
  notifySuccess: mocks.notifySuccess,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { issueKeys } from "../api/keys.js";
import {
  attachCreatedIssue,
  LIST_CYCLE_FAILED_TITLE,
  LIST_CREATED_TITLE,
  LIST_MODULE_FAILED_TITLE,
  LIST_PROPERTY_FAILED_TITLE,
  useListCycleMove,
  useListIssuePatch,
  useListModuleWrite,
} from "./mutations.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
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

function page(rows: IssueListItem[]) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: rows.length,
    total_results: rows.length,
    results: rows,
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function setupQueryClient() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClient.setQueryData(issueKeys.lists(SLUG, PROJECT, "page", "flat"), page([row()]));
  return queryClient;
}

function cachedName(queryClient: QueryClient): string | undefined {
  const data = queryClient.getQueryData<{ results: IssueListItem[] }>(issueKeys.lists(SLUG, PROJECT, "page", "flat"));
  return data?.results[0]?.name;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.updateIssue.mockResolvedValue(undefined);
  mocks.addCycle.mockResolvedValue(undefined);
  mocks.removeCycle.mockResolvedValue(undefined);
  mocks.setModules.mockResolvedValue(undefined);
});

afterEach(() => vi.restoreAllMocks());

describe("useListIssuePatch", () => {
  it("paints the field across cached copies before the write settles", async () => {
    let release: (() => void) | undefined;
    mocks.updateIssue.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListIssuePatch(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.patchIssue({ name: "Renamed" });
    await waitFor(() => expect(cachedName(queryClient)).toBe("Renamed"));
    expect(mocks.updateIssue).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, ISSUE, { name: "Renamed" });
    release?.();
    await waitFor(() => expect(result.current.isSaving).toBe(false));
  });

  it("rolls back and notifies when the write fails", async () => {
    mocks.updateIssue.mockRejectedValue(new Error("offline"));
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListIssuePatch(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.patchIssue({ priority: "high" });
    await waitFor(() => expect(mocks.notifyError).toHaveBeenCalledWith(LIST_PROPERTY_FAILED_TITLE, expect.any(String)));
    expect(cachedName(queryClient)).toBe("Fix login");
    const cached = queryClient.getQueryData<{ results: IssueListItem[] }>(
      issueKeys.lists(SLUG, PROJECT, "page", "flat")
    );
    expect(cached?.results[0]?.priority).toBe("none");
  });
});

describe("useListCycleMove", () => {
  it("adds the issue to the next cycle and paints it", async () => {
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListCycleMove(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveToCycle(CYCLE, null);
    await waitFor(() => expect(mocks.addCycle).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, CYCLE, ISSUE));
    const cached = queryClient.getQueryData<{ results: IssueListItem[] }>(
      issueKeys.lists(SLUG, PROJECT, "page", "flat")
    );
    expect(cached?.results[0]?.cycle_id).toBe(CYCLE);
    expect(mocks.removeCycle).not.toHaveBeenCalled();
  });

  it("removes the issue from its current cycle when cleared", async () => {
    const queryClient = setupQueryClient();
    queryClient.setQueryData(issueKeys.lists(SLUG, PROJECT, "page", "flat"), page([row({ cycle_id: OTHER_CYCLE })]));
    const { result } = renderHook(() => useListCycleMove(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveToCycle(null, OTHER_CYCLE);
    await waitFor(() =>
      expect(mocks.removeCycle).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, OTHER_CYCLE, ISSUE)
    );
    expect(mocks.addCycle).not.toHaveBeenCalled();
  });

  it("does no bridge write when both sides are null", async () => {
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListCycleMove(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveToCycle(null, null);
    await waitFor(() => expect(result.current.isSaving).toBe(false));
    expect(mocks.addCycle).not.toHaveBeenCalled();
    expect(mocks.removeCycle).not.toHaveBeenCalled();
  });

  it("rolls back and notifies when the cycle write fails", async () => {
    mocks.addCycle.mockRejectedValue(new Error("offline"));
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListCycleMove(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveToCycle(CYCLE, null);
    await waitFor(() => expect(mocks.notifyError).toHaveBeenCalledWith(LIST_CYCLE_FAILED_TITLE, expect.any(String)));
    const cached = queryClient.getQueryData<{ results: IssueListItem[] }>(
      issueKeys.lists(SLUG, PROJECT, "page", "flat")
    );
    expect(cached?.results[0]?.cycle_id).toBeNull();
  });
});

describe("useListModuleWrite", () => {
  it("writes module membership with removals", async () => {
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListModuleWrite(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.writeModules([MODULE], ["removed"]);
    await waitFor(() =>
      expect(mocks.setModules).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, ISSUE, {
        modules: [MODULE],
        removed_modules: ["removed"],
      })
    );
    const cached = queryClient.getQueryData<{ results: IssueListItem[] }>(
      issueKeys.lists(SLUG, PROJECT, "page", "flat")
    );
    expect(cached?.results[0]?.module_ids).toEqual([MODULE]);
  });

  it("rolls back and notifies when the module write fails", async () => {
    mocks.setModules.mockRejectedValue(new Error("offline"));
    const queryClient = setupQueryClient();
    const { result } = renderHook(() => useListModuleWrite(SLUG, PROJECT, ISSUE), {
      wrapper: wrapper(queryClient),
    });
    result.current.writeModules([MODULE]);
    await waitFor(() => expect(mocks.notifyError).toHaveBeenCalledWith(LIST_MODULE_FAILED_TITLE, expect.any(String)));
    const cached = queryClient.getQueryData<{ results: IssueListItem[] }>(
      issueKeys.lists(SLUG, PROJECT, "page", "flat")
    );
    expect(cached?.results[0]?.module_ids).toEqual([]);
  });
});

describe("attachCreatedIssue", () => {
  it("attaches to a cycle and a module and toasts", async () => {
    await attachCreatedIssue({ stub: true } as never, SLUG, PROJECT, ISSUE, { cycleId: CYCLE, moduleId: MODULE });
    expect(mocks.addCycle).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, CYCLE, ISSUE);
    expect(mocks.setModules).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, ISSUE, { modules: [MODULE] });
    expect(mocks.notifySuccess).toHaveBeenCalledWith(LIST_CREATED_TITLE);
  });

  it("only attaches what the target names", async () => {
    await attachCreatedIssue({ stub: true } as never, SLUG, PROJECT, ISSUE, {});
    expect(mocks.addCycle).not.toHaveBeenCalled();
    expect(mocks.setModules).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).toHaveBeenCalledWith(LIST_CREATED_TITLE);
  });

  it("surfaces attach failures to the caller", async () => {
    mocks.addCycle.mockRejectedValue(new Error("bridge down"));
    await expect(attachCreatedIssue({ stub: true } as never, SLUG, PROJECT, ISSUE, { cycleId: CYCLE })).rejects.toThrow(
      "bridge down"
    );
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
  });
});
