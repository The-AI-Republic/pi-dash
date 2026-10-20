// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edit and uninstall mutations (parity/specs/agents.md §3, AGT-010/012):
// both refresh the install list and the catalog, both raise the shared
// failure notice, and the server's field detail wins over a generic line.
import { QueryClient, QueryClientProvider, type InvalidateQueryFilters } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { ApiError, type SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts, notify } = vi.hoisted(() => ({
  contracts: { deleteSchedulerBinding: vi.fn(), updateSchedulerBinding: vi.fn() },
  notify: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: notify.error, notifySuccess: notify.success }));

import {
  bindingKeys,
  schedulerBindingFailure,
  schedulerCatalogKey,
  useUninstallSchedulerBinding,
  useUpdateSchedulerBinding,
} from "./installData.js";

const SLUG = "acme";
const PROJECT = "p1";

function binding(): SchedulerBinding {
  return { id: "b1" } as SchedulerBinding;
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function client(): {
  queryClient: QueryClient;
  invalidate: MockInstance<(filters?: InvalidateQueryFilters) => Promise<void>>;
} {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [binding()]);
  return {
    queryClient,
    invalidate: vi.spyOn(queryClient, "invalidateQueries") as unknown as MockInstance<
      (filters?: InvalidateQueryFilters) => Promise<void>
    >,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => {
  resetAppClient();
});

describe("useUpdateSchedulerBinding", () => {
  it("sends the PATCH, notices the save and refreshes list plus catalog", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue(binding());
    const { queryClient, invalidate } = client();
    const { result } = renderHook(() => useUpdateSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1", input: { tzid: "Asia/Tokyo" } });
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1", {
      tzid: "Asia/Tokyo",
    });
    expect(notify.success).toHaveBeenCalledWith("Install updated", "Subsequent runs use the new settings.");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerCatalogKey(SLUG) });
  });

  it("raises the server's field detail when refused", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 400, code: "invalid", message: "Bad Request", fields: { pod: ["Unknown pod."] } })
    );
    const { queryClient } = client();
    const { result } = renderHook(() => useUpdateSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await result.current
      .mutateAsync({ projectId: PROJECT, bindingId: "b1", input: { pod: "gone" } })
      .catch(() => undefined);
    expect(notify.error).toHaveBeenCalledWith("Something went wrong", "Unknown pod.");
  });
});

describe("useUninstallSchedulerBinding", () => {
  it("sends the DELETE, notices the removal and refreshes list plus catalog", async () => {
    contracts.deleteSchedulerBinding.mockResolvedValue(204);
    const { queryClient, invalidate } = client();
    const { result } = renderHook(() => useUninstallSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1" });
    expect(contracts.deleteSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1");
    expect(notify.success).toHaveBeenCalledWith(
      "Scheduler uninstalled",
      "It stops firing on this project until reinstalled."
    );
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerCatalogKey(SLUG) });
  });

  it("raises a failure notice when the install is already gone", async () => {
    contracts.deleteSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 404, code: "not_found", message: "No such install." })
    );
    const { queryClient } = client();
    const { result } = renderHook(() => useUninstallSchedulerBinding(SLUG), { wrapper: wrapper(queryClient) });
    await result.current.mutateAsync({ projectId: PROJECT, bindingId: "b1" }).catch(() => undefined);
    expect(notify.error).toHaveBeenCalledWith("Something went wrong", "No such install.");
  });
});

describe("schedulerBindingFailure", () => {
  it("walks the binding fields in order and falls back to the message", () => {
    const fields = (...names: string[]): ApiError =>
      new ApiError({
        status: 400,
        code: "invalid",
        message: "Bad Request",
        fields: Object.fromEntries(names.map((name) => [name, [`${name} is wrong.`]])),
      });
    expect(schedulerBindingFailure(fields("enabled", "pod"))).toBe("pod is wrong.");
    expect(schedulerBindingFailure(fields("outcome_mode"))).toBe("outcome_mode is wrong.");
    expect(schedulerBindingFailure(fields("rrule"))).toBe("rrule is wrong.");
    expect(schedulerBindingFailure(fields("other"))).toBe("Bad Request");
    expect(schedulerBindingFailure(new ApiError({ status: 500, code: "oops", message: "  " }))).toBe(
      "The install was refused."
    );
    expect(schedulerBindingFailure(new Error("Offline"))).toBe("Check your connection and try again.");
  });
});
