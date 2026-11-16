// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board card writes (parity/specs/issues.md §5, ISS-037–039): the PATCH
// plus the cycle/module bridge a drop carries, the optimistic paint and
// its rollback, and the entity lists refreshed on settle.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateIssue: vi.fn(),
  addCycle: vi.fn(),
  removeCycle: vi.fn(),
  setModules: vi.fn(),
  notifyError: vi.fn(),
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
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { issueKeys } from "../api/keys.js";
import type { IssuesEntity } from "../prefs/entities.js";
import type { BoardMovePlan } from "./drag.js";
import { BOARD_MOVE_FAILED_TITLE, movePaint, nextModuleIds, useBoardCardMove } from "./mutations.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const DONE = "423e4567-e89b-12d3-a456-426614174003";
const CYCLE = "723e4567-e89b-12d3-a456-426614174006";
const MODULE = "823e4567-e89b-12d3-a456-426614174007";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };

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

function plan(overrides: Partial<BoardMovePlan> = {}): BoardMovePlan {
  return {
    patch: { state_id: DONE, sort_order: 100 },
    cycle: null,
    modules: null,
    reorderOnly: false,
    ...overrides,
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.updateIssue.mockResolvedValue(undefined);
  mocks.addCycle.mockResolvedValue({ message: "ok" });
  mocks.removeCycle.mockResolvedValue(undefined);
  mocks.setModules.mockResolvedValue({ message: "ok" });
});

describe("nextModuleIds", () => {
  it("keeps the untouched memberships around the plan's pair", () => {
    expect(nextModuleIds(["keep", "gone"], plan({ modules: { add: ["next"], remove: ["gone"] } }))).toEqual([
      "keep",
      "next",
    ]);
  });

  it("copies the set without a module leg", () => {
    expect(nextModuleIds(["keep"], plan())).toEqual(["keep"]);
  });
});

describe("movePaint", () => {
  it("paints the patch with the bridged memberships", () => {
    expect(movePaint(plan({ cycle: { add: CYCLE, remove: null } }), [])).toEqual({
      state_id: DONE,
      sort_order: 100,
      cycle_id: CYCLE,
    });
    expect(movePaint(plan({ cycle: { add: null, remove: CYCLE } }), [])["cycle_id"]).toBeNull();
    expect(movePaint(plan({ modules: { add: [MODULE], remove: [] } }), [])["module_ids"]).toEqual([MODULE]);
  });
});

describe("useBoardCardMove", () => {
  it("patches the row and refreshes the entity lists", async () => {
    const queryClient = new QueryClient();
    const key = issueKeys.lists(SLUG, PROJECT, "board");
    queryClient.setQueryData(key, { results: [row()] });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({ issueId: ISSUE, plan: plan(), currentModuleIds: [] });
    await waitFor(() => expect(mocks.updateIssue).toHaveBeenCalledTimes(1));
    expect(mocks.updateIssue).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, ISSUE, plan().patch);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.lists(SLUG, PROJECT) }));
    expect(result.current.isMoving).toBe(false);
  });

  it("paints the card before the write lands", async () => {
    const queryClient = new QueryClient();
    const key = issueKeys.lists(SLUG, PROJECT, "board");
    queryClient.setQueryData(key, { results: [row()] });
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    mocks.updateIssue.mockImplementationOnce(async () => {
      await gate;
    });
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({ issueId: ISSUE, plan: plan(), currentModuleIds: [] });
    await waitFor(() => expect(result.current.isMoving).toBe(true));
    const painted = queryClient.getQueryData<{ results: IssueListItem[] }>(key);
    expect(painted?.results[0]?.state_id).toBe(DONE);
    release();
    await waitFor(() => expect(result.current.isMoving).toBe(false));
  });

  it("adds through the cycle bridge", async () => {
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({
      issueId: ISSUE,
      plan: plan({ patch: { sort_order: 100 }, cycle: { add: CYCLE, remove: null } }),
      currentModuleIds: [],
    });
    await waitFor(() => expect(mocks.addCycle).toHaveBeenCalledTimes(1));
    expect(mocks.addCycle).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, CYCLE, ISSUE);
    expect(mocks.removeCycle).not.toHaveBeenCalled();
  });

  it("removes through the cycle bridge", async () => {
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({
      issueId: ISSUE,
      plan: plan({ patch: { sort_order: 100 }, cycle: { add: null, remove: CYCLE } }),
      currentModuleIds: [],
    });
    await waitFor(() => expect(mocks.removeCycle).toHaveBeenCalledTimes(1));
    expect(mocks.removeCycle).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, CYCLE, ISSUE);
  });

  it("writes module membership with removals only when something leaves", async () => {
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({
      issueId: ISSUE,
      plan: plan({ patch: { sort_order: 100 }, modules: { add: [MODULE], remove: ["gone"] } }),
      currentModuleIds: ["gone"],
    });
    await waitFor(() => expect(mocks.setModules).toHaveBeenCalledTimes(1));
    expect(mocks.setModules).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, ISSUE, {
      modules: [MODULE],
      removed_modules: ["gone"],
    });
  });

  it("omits the removals key when nothing leaves", async () => {
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({
      issueId: ISSUE,
      plan: plan({ patch: { sort_order: 100 }, modules: { add: [MODULE], remove: [] } }),
      currentModuleIds: [],
    });
    await waitFor(() => expect(mocks.setModules).toHaveBeenCalledTimes(1));
    expect(mocks.setModules).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, ISSUE, {
      modules: [MODULE],
    });
  });

  it("rolls the paint back and notifies when the write is refused", async () => {
    const queryClient = new QueryClient();
    const key = issueKeys.lists(SLUG, PROJECT, "board");
    queryClient.setQueryData(key, { results: [row()] });
    mocks.updateIssue.mockRejectedValueOnce(new Error("denied"));
    const { result } = renderHook(() => useBoardCardMove(SLUG, PROJECT, ENTITY), {
      wrapper: wrapper(queryClient),
    });
    result.current.moveCard({ issueId: ISSUE, plan: plan(), currentModuleIds: [] });
    await waitFor(() => expect(mocks.notifyError).toHaveBeenCalledTimes(1));
    expect(mocks.notifyError).toHaveBeenCalledWith(BOARD_MOVE_FAILED_TITLE, expect.any(String));
    const rolledBack = queryClient.getQueryData<{ results: IssueListItem[] }>(key);
    expect(rolledBack?.results[0]?.state_id).toBeNull();
  });
});
