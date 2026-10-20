// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    createSchedulerBinding: vi.fn(),
    listSchedulerBindings: vi.fn(),
    updateSchedulerBinding: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  bindingKeys,
  bindingsQueryOptions,
  invalidateInstalls,
  schedulerCatalogKey,
  useInstallSchedulerBinding,
  useSchedulerBindings,
  useToggleSchedulerBinding,
} from "./installData.js";

const SLUG = "acme";
const PROJECT = "p1";

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly",
    scheduler_name: "Weekly",
    scheduler_color: "#5e6ad2",
    project: PROJECT,
    workspace: "w1",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "UTC",
    rrule: "FREQ=DAILY",
    rdates: [],
    exdates: [],
    extra_context: "",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: "2026-10-20T09:00:00Z",
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
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
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => resetAppClient());

describe("binding keys and query", () => {
  it("scopes the list by workspace and project", () => {
    expect(bindingKeys.list("acme", "p1")).toEqual(["ws", "acme", "scheduler-bindings", "p1", "list"]);
    expect(schedulerCatalogKey("acme")).toEqual(["ws", "acme", "schedulers", "list"]);
  });

  it("reads the project installs through the contract", async () => {
    const rows = [binding()];
    contracts.listSchedulerBindings.mockResolvedValue(rows);
    const client = { getJson: vi.fn(), parse: (_schema: unknown, data: unknown) => data } as never;
    const options = bindingsQueryOptions(client, SLUG, PROJECT);
    expect(options.queryKey).toEqual(bindingKeys.list(SLUG, PROJECT));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    const [clientArg, slug, projectId] = contracts.listSchedulerBindings.mock.calls[0] as unknown[];
    expect(clientArg).toBe(client);
    expect([slug, projectId]).toEqual([SLUG, PROJECT]);
  });

  it("subscribes a hook to the same query", async () => {
    contracts.listSchedulerBindings.mockResolvedValue([binding()]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useSchedulerBindings(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toHaveLength(1);
  });
});

describe("install mutation", () => {
  it("creates the install and invalidates the project list and catalog", async () => {
    contracts.createSchedulerBinding.mockResolvedValue(binding());
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useInstallSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    const input = { scheduler: "s1", dtstart: "2026-10-20T09:00:00Z", rrule: "FREQ=DAILY" };
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, input });
    });
    expect(contracts.createSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, input);
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(2));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerCatalogKey(SLUG) });
  });
});

describe("enable switch", () => {
  it("flips the row optimistically and settles on the server answer", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue({ ...binding(), enabled: false });
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [binding()]);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });

    let promise: Promise<void> = Promise.resolve();
    act(() => {
      promise = result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: false });
    });
    await waitFor(() =>
      expect(queryClient.getQueryData<SchedulerBinding[]>(bindingKeys.list(SLUG, PROJECT))?.[0]?.enabled).toBe(false)
    );
    await act(async () => {
      await promise;
    });
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1", {
      enabled: false,
    });
    expect(notifySuccess).toHaveBeenCalledWith("Install updated");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerCatalogKey(SLUG) });
  });

  it("rolls the row back and notifies when the write is refused", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(new Error("Admins only"));
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [binding()]);
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: false }).catch(() => undefined);
    });
    expect(queryClient.getQueryData<SchedulerBinding[]>(bindingKeys.list(SLUG, PROJECT))?.[0]?.enabled).toBe(true);
    expect(notifyError).toHaveBeenCalledWith("Could not update the install", "Admins only");
  });

  it("leaves an unsettled list unsettled after a successful flip", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue({ ...binding(), enabled: true });
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData<SchedulerBinding[] | undefined>(bindingKeys.list(SLUG, PROJECT), undefined);
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: true });
    });
    expect(notifySuccess).toHaveBeenCalledWith("Install updated");
  });

  it("keeps a missing cache empty when the optimistic write runs cold", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue({ ...binding(), enabled: true });
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: true });
    });
    expect(queryClient.getQueryData(bindingKeys.list(SLUG, PROJECT))).toBeUndefined();
  });

  it("leaves rows it does not own untouched during the optimistic flip", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue({ ...binding({ id: "b2" }), enabled: false });
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [binding(), binding({ id: "b2" })]);
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    let promise: Promise<void> = Promise.resolve();
    act(() => {
      promise = result.current.mutateAsync({ projectId: PROJECT, bindingId: "b2", enabled: false });
    });
    await waitFor(() =>
      expect(queryClient.getQueryData<SchedulerBinding[]>(bindingKeys.list(SLUG, PROJECT))?.[1]?.enabled).toBe(false)
    );
    const rows = queryClient.getQueryData<SchedulerBinding[]>(bindingKeys.list(SLUG, PROJECT));
    expect(rows?.[0]?.enabled).toBe(true);
    await act(async () => {
      await promise;
    });
  });
});

describe("invalidateInstalls", () => {
  it("refreshes both subtrees", async () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue();
    await invalidateInstalls(queryClient, SLUG, PROJECT);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerCatalogKey(SLUG) });
  });
});
