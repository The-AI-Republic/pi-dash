// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { pickerKeys } from "../../../shared/pickers/index.js";
import { issueKeys } from "./keys.js";
import {
  CYCLE_ASSIGNED_TITLE,
  CYCLE_ASSIGN_FAILED_BODY,
  CYCLE_REMOVE_FAILED_BODY,
  CYCLE_REMOVED_TITLE,
  CYCLE_UPDATE_FAILED_TITLE,
  MODULE_UPDATE_FAILED_TITLE,
  useIssueCycleWrite,
  useIssueModulesWrite,
} from "./membership.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const OTHER_CYCLE = "523e4567-e89b-12d3-a456-426614174004";
const MODULE_A = "623e4567-e89b-12d3-a456-426614174005";
const MODULE_B = "723e4567-e89b-12d3-a456-426614174006";

interface Call {
  url: string;
  method: string;
  body: string;
}

interface StubOptions {
  status?: number;
  hold?: { release: () => void } | null;
}

function stubClient(options: StubOptions = {}) {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "c" }) };
    }
    if (options.hold) await new Promise<void>((resolve) => (options.hold!.release = resolve));
    const status = options.status ?? 201;
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (status >= 400 ? JSON.stringify({ error: "refused" }) : JSON.stringify({ message: "ok" })),
    };
  }) as never);
  return { calls };
}

interface Handle {
  cycle: { isSavingCycle: boolean; updateCycle: (id: string | null) => void } | null;
  modules: { isSavingModules: boolean; updateModules: (ids: string[], previous: string[]) => void } | null;
}

function Probe({ currentCycleId, withModules }: { currentCycleId: string | null; withModules: boolean }) {
  const cycle = useIssueCycleWrite({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE, currentCycleId });
  const modules = useIssueModulesWrite({ workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE });
  handleRef.cycle = cycle;
  handleRef.modules = modules;
  return (
    <span
      data-cycle-saving={cycle.isSavingCycle ? "true" : "false"}
      data-module-saving={modules.isSavingModules ? "true" : "false"}
      data-with-modules={withModules ? "true" : "false"}
    />
  );
}

let handleRef: Handle = { cycle: null, modules: null };

const detailKey = issueKeys.detail(SLUG, ISSUE);
const listKey = [...issueKeys.all(SLUG), "list", PROJECT, {}, "sort_order"] as const;

function detail(overrides: Record<string, unknown> = {}) {
  return { id: ISSUE, name: "Fix login", cycle_id: null, module_ids: [], ...overrides };
}

function setup(options: StubOptions = {}, currentCycleId: string | null = CYCLE) {
  handleRef = { cycle: null, modules: null };
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
  const { calls } = stubClient(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(detailKey, detail({ cycle_id: currentCycleId, module_ids: [MODULE_A] }));
  queryClient.setQueryData([...issueKeys.all(SLUG), "list", "grouped"], {
    count: 1,
    results: [detail({ cycle_id: currentCycleId, module_ids: [MODULE_A] }), { id: "other" }],
  });
  queryClient.setQueryData(listKey, {
    count: 1,
    results: [detail({ cycle_id: currentCycleId, module_ids: [MODULE_A] }), { id: "other" }],
  });
  queryClient.setQueryData(pickerKeys.cycles(SLUG, PROJECT), [{ id: CYCLE }]);
  queryClient.setQueryData(pickerKeys.modules(SLUG, PROJECT), [{ id: MODULE_A }]);
  render(
    <QueryClientProvider client={queryClient}>
      <Probe currentCycleId={currentCycleId} withModules />
    </QueryClientProvider>
  );
  const patchCalls = () => calls.filter((call) => call.method !== "GET");
  return { calls, patchCalls, queryClient };
}

afterEach(() => {
  resetAppClient();
});

describe("useIssueCycleWrite", () => {
  it("no-ops when the picked cycle is the current one", async () => {
    const { calls } = setup();
    handleRef.cycle!.updateCycle(CYCLE);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls.filter((call) => call.method !== "GET")).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("assigns through the cycle bridge and confirms", async () => {
    const { patchCalls, queryClient } = setup();
    handleRef.cycle!.updateCycle(OTHER_CYCLE);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(patchCalls()[0]!.method).toBe("POST");
    expect(patchCalls()[0]!.url).toContain(`/cycles/${OTHER_CYCLE}/cycle-issues/`);
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ issues: [ISSUE] });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CYCLE_ASSIGNED_TITLE));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ cycle_id: OTHER_CYCLE });
  });

  it("paints the new cycle before the bridge settles", async () => {
    const hold: { release: () => void } = { release: () => {} };
    const { queryClient } = setup({ hold });
    handleRef.cycle!.updateCycle(OTHER_CYCLE);
    await waitFor(() => expect(queryClient.getQueryData(detailKey)).toMatchObject({ cycle_id: OTHER_CYCLE }));
    expect(handleRef.cycle!.isSavingCycle).toBe(true);
    hold.release();
    await waitFor(() => expect(handleRef.cycle!.isSavingCycle).toBe(false));
  });

  it("clears through the bridge delete and confirms", async () => {
    const { patchCalls } = setup();
    handleRef.cycle!.updateCycle(null);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(patchCalls()[0]!.method).toBe("DELETE");
    expect(patchCalls()[0]!.url).toContain(`/cycles/${CYCLE}/cycle-issues/${ISSUE}/`);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CYCLE_REMOVED_TITLE));
  });

  it("sends nothing when clearing an item with no cycle", async () => {
    const { calls } = setup({}, null);
    handleRef.cycle!.updateCycle(null);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls.filter((call) => call.method !== "GET")).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("rolls back and reports a refused assignment", async () => {
    const { queryClient } = setup({ status: 500 });
    handleRef.cycle!.updateCycle(OTHER_CYCLE);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CYCLE_UPDATE_FAILED_TITLE, CYCLE_ASSIGN_FAILED_BODY));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ cycle_id: CYCLE });
    expect(queryClient.getQueryData(listKey)).toMatchObject({
      results: expect.arrayContaining([expect.objectContaining({ id: ISSUE, cycle_id: CYCLE })]),
    });
  });

  it("reports a refused removal with the removal copy", async () => {
    const { queryClient } = setup({ status: 500 });
    handleRef.cycle!.updateCycle(null);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(CYCLE_UPDATE_FAILED_TITLE, CYCLE_REMOVE_FAILED_BODY));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ cycle_id: CYCLE });
  });

  it("patches row lists cached under the workspace scope and leaves foreign entries alone", async () => {
    const { queryClient } = setup();
    const rowsKey = [...issueKeys.all(SLUG), "list", "rows"] as const;
    const foreignKey = [...issueKeys.all(SLUG), "list", "foreign"] as const;
    queryClient.setQueryData(rowsKey, [detail({ cycle_id: CYCLE, module_ids: [MODULE_A] }), { id: "other" }]);
    queryClient.setQueryData(foreignKey, "not a row");
    handleRef.cycle!.updateCycle(OTHER_CYCLE);
    await waitFor(() =>
      expect(queryClient.getQueryData(rowsKey)).toEqual([
        expect.objectContaining({ id: ISSUE, cycle_id: OTHER_CYCLE }),
        { id: "other" },
      ])
    );
    expect(queryClient.getQueryData(foreignKey)).toBe("not a row");
  });

  it("re-reads the cycle list after the bridge settles", async () => {
    const { queryClient } = setup();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    handleRef.cycle!.updateCycle(OTHER_CYCLE);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: pickerKeys.cycles(SLUG, PROJECT) }));
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: issueKeys.all(SLUG) }));
  });
});

describe("useIssueModulesWrite", () => {
  it("diffs the selection into the add/remove pair", async () => {
    const { patchCalls, queryClient } = setup();
    handleRef.modules!.updateModules([MODULE_B, MODULE_A], [MODULE_A]);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(patchCalls()[0]!.method).toBe("POST");
    expect(patchCalls()[0]!.url).toContain(`/issues/${ISSUE}/modules/`);
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ modules: [MODULE_B] });
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ module_ids: [MODULE_B, MODULE_A] });
  });

  it("sends the removed ids when the selection shrinks", async () => {
    const { patchCalls } = setup();
    handleRef.modules!.updateModules([], [MODULE_A]);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ modules: [], removed_modules: [MODULE_A] });
  });

  it("sends both sides for a swap", async () => {
    const { patchCalls } = setup();
    handleRef.modules!.updateModules([MODULE_B], [MODULE_A]);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ modules: [MODULE_B], removed_modules: [MODULE_A] });
  });

  it("locks the control while the write is in flight and paints at once", async () => {
    const hold: { release: () => void } = { release: () => {} };
    const { queryClient } = setup({ hold });
    handleRef.modules!.updateModules([MODULE_B], [MODULE_A]);
    await waitFor(() => expect(queryClient.getQueryData(detailKey)).toMatchObject({ module_ids: [MODULE_B] }));
    expect(handleRef.modules!.isSavingModules).toBe(true);
    hold.release();
    await waitFor(() => expect(handleRef.modules!.isSavingModules).toBe(false));
  });

  it("rolls back and reports a refused write", async () => {
    const { queryClient } = setup({ status: 500 });
    handleRef.modules!.updateModules([MODULE_B], [MODULE_A]);
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        MODULE_UPDATE_FAILED_TITLE,
        "The modules are unchanged — check your connection and try again."
      )
    );
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ module_ids: [MODULE_A] });
  });

  it("re-reads the module list after the write settles", async () => {
    const { queryClient } = setup();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    handleRef.modules!.updateModules([MODULE_B], [MODULE_A]);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: pickerKeys.modules(SLUG, PROJECT) }));
  });
});
