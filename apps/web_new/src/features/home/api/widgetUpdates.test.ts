// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePreferenceList } from "@pidash/api-client";

import { widgetPrefsKeys } from "./widgetPrefs.js";
import { reorderWidgetMutationOptions, toggleWidgetMutationOptions } from "./widgetUpdates.js";

const { notifyErrorMock, notifySuccessMock } = vi.hoisted(() => ({
  notifyErrorMock: vi.fn(),
  notifySuccessMock: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notifyErrorMock, notifySuccess: notifySuccessMock };
});

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

function stubClient() {
  const sendJson = vi.fn(async () => ({ data: {}, status: 200, url: "https://api.test/" }));
  const client = { sendJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, sendJson };
}

function pref(key: string, sort_order: number, is_enabled: boolean): HomePreferenceList[number] {
  return { key: key as "recents", is_enabled, config: {}, sort_order };
}

function flags(list: HomePreferenceList | undefined): boolean[] {
  return (list ?? []).map((entry) => entry.is_enabled);
}

function sorts(list: HomePreferenceList | undefined): number[] {
  return (list ?? []).map((entry) => entry.sort_order);
}

describe("toggleWidgetMutationOptions", () => {
  it("patches the toggle through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = toggleWidgetMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.({ key: "recents", is_enabled: false }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("PATCH", "/api/workspaces/acme/home-preferences/recents/", {
      json: { is_enabled: false },
    });
  });

  it("flips the toggle and leaves other rows alone", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("quick_links", 100, true), pref("recents", 99, true)]);
    const options = toggleWidgetMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.({ key: "recents", is_enabled: false }, {} as never);
    expect(flags(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([true, false]);
  });

  it("tolerates an empty cache on mutate", async () => {
    const queryClient = new QueryClient();
    const options = toggleWidgetMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ key: "recents", is_enabled: false }, {} as never);
    expect(queryClient.getQueryData(widgetPrefsKeys.list("acme"))).toBeUndefined();
    options.onError?.(new Error("denied"), { key: "recents", is_enabled: false }, context, {} as never);
    expect(queryClient.getQueryData(widgetPrefsKeys.list("acme"))).toBeUndefined();
  });

  it("rolls back and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("recents", 99, true)]);
    const options = toggleWidgetMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ key: "recents", is_enabled: false }, {} as never);
    expect(flags(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([false]);
    options.onError?.(new Error("denied"), { key: "recents", is_enabled: false }, context, {} as never);
    expect(flags(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([true]);
    expect(notifyErrorMock).toHaveBeenCalledWith("Widget toggle failed", "denied");
  });

  it("toasts without a description for non-error failures", () => {
    const options = toggleWidgetMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onError?.("boom", { key: "recents", is_enabled: false }, undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Widget toggle failed", undefined);
  });

  it("stays silent on success", () => {
    const options = toggleWidgetMutationOptions(stubClient().client, new QueryClient(), "acme");
    expect(options.onSuccess).toBeUndefined();
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("refreshes the map once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = toggleWidgetMutationOptions(stubClient().client, queryClient, "acme");
    options.onSettled?.(undefined, null, { key: "recents", is_enabled: false }, undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: widgetPrefsKeys.all("acme") });
  });
});

describe("reorderWidgetMutationOptions", () => {
  it("patches the position through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = reorderWidgetMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.({ key: "quick_links", sort_order: 98 }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("PATCH", "/api/workspaces/acme/home-preferences/quick_links/", {
      json: { sort_order: 98 },
    });
  });

  it("moves the position and leaves other rows alone", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("quick_links", 100, true), pref("recents", 99, true)]);
    const options = reorderWidgetMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.({ key: "quick_links", sort_order: 98 }, {} as never);
    expect(sorts(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([98, 99]);
  });

  it("tolerates an empty cache on mutate", async () => {
    const queryClient = new QueryClient();
    const options = reorderWidgetMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ key: "quick_links", sort_order: 98 }, {} as never);
    expect(queryClient.getQueryData(widgetPrefsKeys.list("acme"))).toBeUndefined();
    options.onError?.(new Error("denied"), { key: "quick_links", sort_order: 98 }, context, {} as never);
    expect(queryClient.getQueryData(widgetPrefsKeys.list("acme"))).toBeUndefined();
  });

  it("rolls back and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("quick_links", 100, true)]);
    const options = reorderWidgetMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ key: "quick_links", sort_order: 98 }, {} as never);
    expect(sorts(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([98]);
    options.onError?.(new Error("denied"), { key: "quick_links", sort_order: 98 }, context, {} as never);
    expect(sorts(queryClient.getQueryData(widgetPrefsKeys.list("acme")))).toEqual([100]);
    expect(notifyErrorMock).toHaveBeenCalledWith("Widget reorder failed", "denied");
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("toasts without a description for non-error failures", () => {
    const options = reorderWidgetMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onError?.("boom", { key: "quick_links", sort_order: 98 }, undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Widget reorder failed", undefined);
  });

  it("confirms the reorder on success", () => {
    const options = reorderWidgetMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onSuccess?.({} as never, { key: "quick_links", sort_order: 98 }, { rollbacks: [] }, {} as never);
    expect(notifySuccessMock).toHaveBeenCalledWith("Widget order saved");
  });

  it("refreshes the map once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = reorderWidgetMutationOptions(stubClient().client, queryClient, "acme");
    options.onSettled?.(undefined, null, { key: "quick_links", sort_order: 98 }, undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: widgetPrefsKeys.all("acme") });
  });
});
