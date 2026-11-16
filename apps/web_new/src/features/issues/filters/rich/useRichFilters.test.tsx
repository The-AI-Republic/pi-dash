// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { notifyError } from "../../../../shared/shell/index.js";
import { issueKeys } from "../../api/keys.js";
import { IssuesLoaderProvider, useIssuesLoader } from "../../loaders/loaderContext.js";
import { entityListsKeyPart, type IssuesEntity, type PrefsRecord } from "../../prefs/entities.js";
import { prefsQueryOptions } from "../../prefs/prefsQuery.js";
import type { RichCondition } from "./expression.js";
import { buildRichFiltersPatch, selectRichConditions, useRichFilters, useUpdateRichFilters } from "./useRichFilters.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };
const RECORD: PrefsRecord = {
  filters: { state: ["todo"] },
  display_filters: { layout: "list" },
  display_properties: { state: true },
  rich_filters: {},
};

const ROW = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  workspace: "123e4567-e89b-12d3-a456-426614174000",
  project: PROJECT,
  user: "123e4567-e89b-12d3-a456-426614174000",
  filters: { state: ["todo"] },
  display_filters: { layout: "list" },
  display_properties: { state: true },
  rich_filters: {},
  preferences: {},
  sort_order: 1,
  created_by: null,
  updated_by: null,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
};

function row(overrides: Partial<RichCondition> = {}): RichCondition {
  return { key: "a", property: "priority", operator: "in", negated: false, value: ["urgent"], ...overrides };
}

function harness(options: { failPatch?: boolean; holdPatch?: boolean } = {}) {
  const calls: Array<{ method: string; url: string; body: unknown }> = [];
  let release = (): void => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const method = init.method ?? "GET";
    if (method === "PATCH") {
      calls.push({ method, url, body: JSON.parse(init.body ?? "{}") });
      if (options.holdPatch === true) await gate;
      if (options.failPatch === true) throw new Error("offline");
      const patch = JSON.parse(init.body ?? "{}") as Partial<PrefsRecord>;
      const next = { ...ROW, ...patch };
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(next) };
    }
    calls.push({ method, url, body: null });
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(ROW) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(prefsQueryOptions(client, "acme", ENTITY).queryKey, RECORD);
  function wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return (
      <QueryClientProvider client={queryClient}>
        <IssuesLoaderProvider>
          <React.Fragment>{children}</React.Fragment>
        </IssuesLoaderProvider>
      </QueryClientProvider>
    );
  }
  return { calls, client, queryClient, wrapper, release };
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

describe("rich filter selectors", () => {
  it("parses the stored expression and patches only rich_filters", () => {
    const record = { ...RECORD, rich_filters: { and: [{ priority__in: ["urgent"] }] } };
    expect(selectRichConditions(record)).toMatchObject([{ property: "priority", value: ["urgent"] }]);
    expect(buildRichFiltersPatch(record, { priority__in: "high" })).toEqual({ rich_filters: { priority__in: "high" } });
  });
});

describe("useRichFilters", () => {
  it("reports no conditions before the preferences read lands", () => {
    const { client } = harness();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, enabled: false } } });
    function Scoped({ children }: { children: React.ReactNode }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children}</React.Fragment>
        </QueryClientProvider>
      );
    }
    const { result } = renderHook(() => useRichFilters("acme", ENTITY), { wrapper: Scoped });
    expect(client).toBeDefined();
    expect(result.current.data).toBeUndefined();
    expect(result.current.conditions).toEqual([]);
  });

  it("reads the seeded conditions", () => {
    const { client } = harness();
    const queryClient = new QueryClient();
    queryClient.setQueryData(prefsQueryOptions(client, "acme", ENTITY).queryKey, {
      ...RECORD,
      rich_filters: { priority__in: "urgent" },
    });
    function Scoped({ children }: { children: React.ReactNode }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children}</React.Fragment>
        </QueryClientProvider>
      );
    }
    const { result } = renderHook(() => useRichFilters("acme", ENTITY), { wrapper: Scoped });
    expect(result.current.conditions).toMatchObject([{ property: "priority", value: "urgent" }]);
  });
});

describe("useUpdateRichFilters", () => {
  it("patches the complete rich_filters object and refetches the list", async () => {
    const { calls, queryClient, wrapper } = harness();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useUpdateRichFilters("acme", ENTITY), { wrapper });

    act(() => result.current.updateRichFilters([row()]));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));

    const patch = calls.find((call) => call.method === "PATCH")!;
    expect(patch.url).toBe(`https://api.test/api/workspaces/acme/projects/${PROJECT}/user-properties/`);
    expect(patch.body).toEqual({ rich_filters: { priority__in: "urgent" } });
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: issueKeys.lists("acme", ...entityListsKeyPart(ENTITY)),
      })
    );
  });

  it("raises the issues loader for the write and lowers it when the write settles", async () => {
    const { release, wrapper } = harness({ holdPatch: true });
    const { result } = renderHook(() => ({ write: useUpdateRichFilters("acme", ENTITY), loader: useIssuesLoader() }), {
      wrapper,
    });
    expect(result.current.loader.isMutationLoading).toBe(false);

    act(() => result.current.write.updateRichFilters([row()]));
    await waitFor(() => expect(result.current.write.isSaving).toBe(true));
    expect(result.current.loader.isMutationLoading).toBe(true);

    release();
    await waitFor(() => expect(result.current.write.isSaving).toBe(false));
    expect(result.current.loader.isMutationLoading).toBe(false);
  });

  it("writes without a mounted loader when the builder sits above the layout root", async () => {
    const { calls, queryClient } = harness();
    function Unloaded({ children }: { children: React.ReactNode }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <React.Fragment>{children}</React.Fragment>
        </QueryClientProvider>
      );
    }
    const { result } = renderHook(() => useUpdateRichFilters("acme", ENTITY), { wrapper: Unloaded });

    act(() => result.current.updateRichFilters([row({ value: "low" })]));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")!.body).toEqual({ rich_filters: { priority__in: "low" } });
  });

  it("rolls the optimistic write back and toasts when the patch fails", async () => {
    const { queryClient, wrapper } = harness({ failPatch: true });
    const { result } = renderHook(() => useUpdateRichFilters("acme", ENTITY), { wrapper });

    act(() => result.current.updateRichFilters([row({ value: "high" })]));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    const cached = queryClient.getQueryData<PrefsRecord>(issueKeys.prefs("acme", "project", PROJECT));
    expect(cached?.rich_filters).toEqual({});
    expect(notifyError).toHaveBeenCalledWith("Couldn't save filters", expect.any(String));
  });
});

describe("disabled rich-filter writes", () => {
  it("no-ops for a disabled builder, such as a saved-view or temporary row", async () => {
    const { calls, wrapper } = harness();
    const { result } = renderHook(() => useUpdateRichFilters("acme", ENTITY, false), { wrapper });
    act(() => result.current.updateRichFilters([row()]));
    expect(result.current.isSaving).toBe(false);
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});
