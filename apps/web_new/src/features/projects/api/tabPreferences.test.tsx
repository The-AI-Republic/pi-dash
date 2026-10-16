// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  applyNavigationPrefs,
  mergeNavigationPrefs,
  selectProjectListCap,
  useTabPreferences,
  type TabPreferencesHandle,
} from "./tabPreferences.js";
import { useProjectProperties } from "./tabPreferences.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function propsPayload(navigation: Record<string, unknown>) {
  return {
    id: UUID,
    workspace: UUID2,
    project: UUID2,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences: { pages: { block_display: false }, navigation },
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

interface StubOptions {
  navigation?: Record<string, unknown>;
  patchStatus?: number;
  hangGet?: boolean;
}

function stubClient(options: StubOptions = {}) {
  let serverNavigation = options.navigation ?? { default_tab: "work_items", hide_in_more_menu: [] };
  const calls: Array<{ url: string; method: string; body: string }> = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      const sent = JSON.parse(init.body ?? "{}") as { preferences?: { navigation?: Record<string, unknown> } };
      if (sent.preferences?.navigation !== undefined) serverNavigation = sent.preferences.navigation;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(propsPayload(serverNavigation)),
      };
    }
    if (options.hangGet === true) return new Promise<never>(() => undefined);
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(propsPayload(serverNavigation)),
    };
  }) as never);
  return { client, calls };
}

function renderHandle(options: StubOptions = {}) {
  const { calls } = stubClient(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let handle: TabPreferencesHandle | null = null;
  function Probe(): React.ReactElement {
    handle = useTabPreferences("acme", UUID2);
    const stored = useProjectProperties("acme", UUID2);
    return (
      <p>
        {stored.status}|{handle.preferences.defaultTab}|{handle.preferences.hiddenTabs.join(",")}
      </p>
    );
  }
  render(
    <QueryClientProvider client={queryClient}>
      <Probe />
    </QueryClientProvider>
  );
  return {
    calls,
    prefsText: () => screen.getByText(/\|/).textContent ?? "",
    handle: () => {
      if (handle === null) throw new Error("handle never mounted");
      return handle;
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("preference payloads", () => {
  it("merges the navigation branch over kept siblings", () => {
    expect(
      mergeNavigationPrefs({ pages: { block_display: false } }, { defaultTab: "cycles", hiddenTabs: ["pages"] })
    ).toEqual({
      pages: { block_display: false },
      navigation: { default_tab: "cycles", hide_in_more_menu: ["pages"] },
    });
  });

  it("applies prefs onto the cached payload and stays empty without one", () => {
    const previous = propsPayload({ default_tab: "work_items", hide_in_more_menu: [] });
    const next = applyNavigationPrefs(previous as never, { defaultTab: "cycles", hiddenTabs: [] });
    expect(next?.preferences).toEqual({
      pages: { block_display: false },
      navigation: { default_tab: "cycles", hide_in_more_menu: [] },
    });
    expect(applyNavigationPrefs(undefined, { defaultTab: "cycles", hiddenTabs: [] })).toBeUndefined();
  });
});

describe("project list cap selector", () => {
  it("reads zero, negative and absent limits as uncapped", () => {
    expect(selectProjectListCap(0)).toBeNull();
    expect(selectProjectListCap(-4)).toBeNull();
    expect(selectProjectListCap(null)).toBeNull();
    expect(selectProjectListCap(undefined)).toBeNull();
    expect(selectProjectListCap(3)).toBe(3);
  });
});

describe("useTabPreferences", () => {
  it("sets a new default optimistically and announces it", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|"));
    h.handle().toggleDefault("cycles");
    // Optimistic: the UI flips before the PATCH settles.
    await waitFor(() => expect(h.prefsText()).toBe("success|cycles|"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Default tab saved", expect.any(String)));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      preferences: {
        pages: { block_display: false },
        navigation: { default_tab: "cycles", hide_in_more_menu: [] },
      },
    });
  });

  it("clears the default back to tracking with its own notice", async () => {
    const h = renderHandle({ navigation: { default_tab: "cycles", hide_in_more_menu: [] } });
    await waitFor(() => expect(h.prefsText()).toBe("success|cycles|"));
    h.handle().toggleDefault("cycles");
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Default tab cleared", expect.any(String)));
  });

  it("hides a tab silently and persists the list", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|"));
    h.handle().hideTab("pages");
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|pages"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}").preferences.navigation.hide_in_more_menu).toEqual(["pages"]);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("keeps an already-hidden tab listed once", async () => {
    const h = renderHandle({ navigation: { default_tab: "work_items", hide_in_more_menu: ["pages"] } });
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|pages"));
    h.handle().hideTab("pages");
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(h.prefsText()).toBe("success|work_items|pages");
  });

  it("restores a hidden tab silently", async () => {
    const h = renderHandle({ navigation: { default_tab: "work_items", hide_in_more_menu: ["pages", "views"] } });
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|pages,views"));
    h.handle().showTab("pages");
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|views"));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("reverts the optimistic write and toasts when the save fails", async () => {
    const h = renderHandle({ patchStatus: 500 });
    await waitFor(() => expect(h.prefsText()).toBe("success|work_items|"));
    h.handle().toggleDefault("cycles");
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't save the tab choice", expect.any(String)));
    expect(h.prefsText()).toBe("success|work_items|");
  });

  it("holds writes until the payload loads", async () => {
    const h = renderHandle({ hangGet: true });
    await screen.findByText("pending|work_items|");
    h.handle().toggleDefault("cycles");
    h.handle().hideTab("pages");
    h.handle().showTab("pages");
    await waitFor(() => expect(h.calls.some((call) => call.method === "GET")).toBe(true));
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifyError).not.toHaveBeenCalled();
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});
