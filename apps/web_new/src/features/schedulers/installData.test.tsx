// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    createSchedulerBinding: vi.fn(),
    deleteSchedulerBinding: vi.fn(),
    getSchedulerBinding: vi.fn(),
    listBindingRuns: vi.fn(),
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
  bindingDetailQueryOptions,
  bindingKeys,
  bindingRunsQueryOptions,
  bindingsQueryOptions,
  invalidateInstalls,
  RUNS_POLL_INTERVAL_MS,
  schedulerCatalogKey,
  useInstallSchedulerBinding,
  useSchedulerBindings,
  useToggleSchedulerBinding,
  schedulerBindingFailure,
  useUninstallSchedulerBinding,
  useUpdateSchedulerBinding,
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

  it("reads one install through the contract", async () => {
    const row = binding();
    contracts.getSchedulerBinding.mockResolvedValue(row);
    const client = { getJson: vi.fn(), parse: (_schema: unknown, data: unknown) => data } as never;
    const options = bindingDetailQueryOptions(client, SLUG, PROJECT, "b1");
    expect(options.queryKey).toEqual(bindingKeys.detail(SLUG, PROJECT, "b1"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(row);
    expect(contracts.getSchedulerBinding).toHaveBeenCalledWith(client, SLUG, PROJECT, "b1");
  });

  it("re-reads run history on the fifteen-second poll", async () => {
    const page = { results: [], total_count: 0, total_pages: 1 };
    contracts.listBindingRuns.mockResolvedValue(page);
    const client = { getJson: vi.fn(), parse: (_schema: unknown, data: unknown) => data } as never;
    const options = bindingRunsQueryOptions(client, SLUG, PROJECT, "b1", 2);
    expect(options.queryKey).toEqual(bindingKeys.runs(SLUG, PROJECT, "b1", 2));
    expect(RUNS_POLL_INTERVAL_MS).toBe(15_000);
    expect(options.refetchInterval).toBe(RUNS_POLL_INTERVAL_MS);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(page);
    expect(contracts.listBindingRuns).toHaveBeenCalledWith(client, SLUG, PROJECT, "b1", { page: 2 });
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

    let promise: Promise<SchedulerBinding> = Promise.resolve(binding());
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
    expect(notifySuccess).toHaveBeenCalledWith("Install updated", "It will not fire until re-enabled.");
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
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Admins only");
  });

  it("flips the open detail optimistically and seeds it from the server answer", async () => {
    const saved = { ...binding({ enabled: false }), resolved_prompt: "", run_count: 0 };
    contracts.updateSchedulerBinding.mockResolvedValue(saved);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"), { ...saved, enabled: true });
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });

    let promise: Promise<unknown> = Promise.resolve();
    act(() => {
      promise = result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: false });
    });
    await waitFor(() =>
      expect(queryClient.getQueryData<{ enabled: boolean }>(bindingKeys.detail(SLUG, PROJECT, "b1"))?.enabled).toBe(
        false
      )
    );
    await act(async () => {
      await promise;
    });
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toEqual(saved);
  });

  it("rolls the open detail back when the write is refused", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(new Error("Admins only"));
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const current = { ...binding({ enabled: true }), resolved_prompt: "", run_count: 0 };
    queryClient.setQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"), current);
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: false }).catch(() => undefined);
    });
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toEqual(current);
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Admins only");
  });

  it("keeps a cold detail cache unset mid-flight, then seeds it on success", async () => {
    let release: (value: unknown) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise((resolve) => {
        release = resolve;
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    let promise: Promise<unknown> = Promise.resolve();
    act(() => {
      promise = result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: false });
    });
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toBeUndefined();
    const saved = { ...binding({ enabled: false }), resolved_prompt: "", run_count: 0 };
    release(saved);
    await act(async () => {
      await promise;
    });
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toEqual(saved);
  });

  it("leaves an unsettled list unsettled after a successful flip", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue({ ...binding(), enabled: true });
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    queryClient.setQueryData<SchedulerBinding[] | undefined>(bindingKeys.list(SLUG, PROJECT), undefined);
    const { result } = renderHook(() => useToggleSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", enabled: true });
    });
    expect(notifySuccess).toHaveBeenCalledWith("Install updated", "It will fire on the next scheduled tick.");
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
    let promise: Promise<SchedulerBinding> = Promise.resolve(binding());
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

describe("edit and uninstall mutations", () => {
  it("saves the edit, seeds the detail cache and refreshes the lists", async () => {
    const saved = binding({ rrule: "FREQ=WEEKLY" });
    contracts.updateSchedulerBinding.mockResolvedValue(saved);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useUpdateSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", input: { rrule: "FREQ=WEEKLY" } });
    });
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1", {
      rrule: "FREQ=WEEKLY",
    });
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toEqual(saved);
    expect(notifySuccess).toHaveBeenCalledWith("Install updated", "Subsequent runs use the new settings.");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
  });

  it("reports the edit refusal and leaves the detail cache alone", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(new Error("Nope"));
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useUpdateSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", input: {} }).catch(() => undefined);
    });
    expect(queryClient.getQueryData(bindingKeys.detail(SLUG, PROJECT, "b1"))).toBeUndefined();
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Check your connection and try again.");
  });

  it("uninstalls the binding and refreshes the lists", async () => {
    contracts.deleteSchedulerBinding.mockResolvedValue(undefined);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useUninstallSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1" });
    });
    expect(contracts.deleteSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1");
    expect(notifySuccess).toHaveBeenCalledWith(
      "Scheduler uninstalled",
      "It stops firing on this project until reinstalled."
    );
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
  });

  it("reports a refused uninstall", async () => {
    contracts.deleteSchedulerBinding.mockRejectedValue(new Error("Admins only"));
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useUninstallSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1" }).catch(() => undefined);
    });
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Check your connection and try again.");
  });
});

describe("schedulerBindingFailure", () => {
  it("prefers the first field message the server sends", () => {
    const error = new ApiError({
      status: 400,
      code: "invalid",
      message: "Bad Request",
      fields: { rrule: ["Rule is invalid."], dtstart: ["Start is invalid."] },
    });
    expect(schedulerBindingFailure(error)).toBe("Rule is invalid.");
  });

  it("falls back to the server message, then to the fixed copy for a blank one", () => {
    expect(
      schedulerBindingFailure(new ApiError({ status: 409, code: "conflict", message: "Already installed." }))
    ).toBe("Already installed.");
    expect(schedulerBindingFailure(new ApiError({ status: 400, code: "invalid", message: "   " }))).toBe(
      "The install was refused."
    );
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
