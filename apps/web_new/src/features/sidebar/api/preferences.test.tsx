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
  applySidebarPatches,
  comparePersonalOrder,
  moveEntryOrder,
  reorderPatches,
  selectPersonalEntries,
  selectPersonalGroup,
  selectVisiblePersonalEntries,
  useSidebarPreferences,
  useUpdateSidebarPreferences,
  type PersonalEntryState,
} from "./preferences.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

type PrefsMap = Record<string, { is_pinned: boolean; sort_order: number }>;

interface StubOptions {
  prefs?: PrefsMap;
  patchStatus?: number;
  hangGet?: boolean;
}

function stubClient(options: StubOptions = {}) {
  let server: PrefsMap = options.prefs ?? {
    drafts: { is_pinned: true, sort_order: 0 },
    your_work: { is_pinned: true, sort_order: 1 },
  };
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
      const sent = JSON.parse(init.body ?? "[]") as Array<{
        key?: string;
        is_pinned?: boolean;
        sort_order?: number;
      }>;
      const next: PrefsMap = { ...server };
      for (const item of sent) {
        if (item.key === undefined || next[item.key] === undefined) continue;
        next[item.key] = {
          is_pinned: item.is_pinned ?? next[item.key]!.is_pinned,
          sort_order: item.sort_order ?? next[item.key]!.sort_order,
        };
      }
      server = next;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ message: "ok" }),
      };
    }
    if (options.hangGet === true) return new Promise<never>(() => undefined);
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(server),
    };
  }) as never);
  return { client, calls };
}

function state(key: string, enabled: boolean, sortOrder: number): PersonalEntryState {
  return { key, label: key, path: key, enabled, sortOrder };
}

describe("sidebar preference selectors", () => {
  it("defaults the catalog to visible without stored preferences", () => {
    expect(selectPersonalEntries(undefined)).toEqual([
      { key: "drafts", label: "Drafts", path: "drafts", enabled: true, sortOrder: 0 },
    ]);
  });

  it("resolves pins and order over storage, ignoring foreign keys", () => {
    expect(
      selectPersonalEntries({
        drafts: { is_pinned: false, sort_order: 5 },
        your_work: { is_pinned: true, sort_order: 0 },
        views: { is_pinned: true, sort_order: 1 },
      })
    ).toEqual([{ key: "drafts", label: "Drafts", path: "drafts", enabled: false, sortOrder: 5 }]);
  });

  it("fills catalog rows missing from storage with defaults", () => {
    expect(selectPersonalEntries({ views: { is_pinned: true, sort_order: 1 } })).toEqual([
      { key: "drafts", label: "Drafts", path: "drafts", enabled: true, sortOrder: 0 },
    ]);
  });

  it("orders personal rows by ascending stored position", () => {
    const rows = [state("b", true, 2), state("a", true, 0), state("c", true, 1)];
    expect([...rows].sort(comparePersonalOrder).map((row) => row.key)).toEqual(["a", "c", "b"]);
  });

  it("lists visible entries with sidebar addresses", () => {
    expect(selectVisiblePersonalEntries([state("drafts", true, 0), state("x", false, 1)], "acme")).toEqual([
      { key: "drafts", label: "drafts", href: "/acme/drafts/" },
    ]);
  });

  it("folds query states into the personal view-model", () => {
    expect(selectPersonalGroup("pending", undefined)).toEqual({ status: "pending", entries: [] });
    expect(selectPersonalGroup("error", undefined)).toEqual({ status: "error", entries: [] });
    expect(selectPersonalGroup("success", undefined).entries).toEqual(selectPersonalEntries(undefined));
    expect(selectPersonalGroup("success", { drafts: { is_pinned: false, sort_order: 3 } }).entries).toEqual([
      { key: "drafts", label: "Drafts", path: "drafts", enabled: false, sortOrder: 3 },
    ]);
  });
});

describe("sidebar reorder math", () => {
  it("moves the dragged key into the target slot", () => {
    expect(moveEntryOrder(["a", "b", "c"], "a", "c")).toEqual(["b", "c", "a"]);
    expect(moveEntryOrder(["a", "b", "c"], "c", "a")).toEqual(["c", "a", "b"]);
    expect(moveEntryOrder(["a", "b", "c"], "a", "b")).toEqual(["b", "a", "c"]);
  });

  it("resolves self-drops and foreign keys back to the current order", () => {
    expect(moveEntryOrder(["a", "b"], "a", "a")).toEqual(["a", "b"]);
    expect(moveEntryOrder(["a", "b"], "z", "a")).toEqual(["a", "b"]);
    expect(moveEntryOrder(["a", "b"], "a", "z")).toEqual(["a", "b"]);
    expect(moveEntryOrder(["drafts"], "drafts", "drafts")).toEqual(["drafts"]);
  });

  it("numbers order triples from zero, or null when nothing moves", () => {
    expect(reorderPatches(["a", "b", "c"], "a", "c")).toEqual([
      { key: "b", sort_order: 0 },
      { key: "c", sort_order: 1 },
      { key: "a", sort_order: 2 },
    ]);
    expect(reorderPatches(["a", "b"], "a", "a")).toBeNull();
    expect(reorderPatches(["a", "b"], "a", "z")).toBeNull();
  });

  it("folds triples over the cached map field by field", () => {
    const previous: PrefsMap = { drafts: { is_pinned: true, sort_order: 4 } };
    expect(applySidebarPatches(previous, [{ key: "drafts", is_pinned: false }])).toEqual({
      drafts: { is_pinned: false, sort_order: 4 },
    });
    expect(applySidebarPatches(previous, [{ key: "drafts", sort_order: 1 }])).toEqual({
      drafts: { is_pinned: true, sort_order: 1 },
    });
    expect(applySidebarPatches(previous, [{ key: "views", is_pinned: true, sort_order: 2 }])).toEqual({
      drafts: { is_pinned: true, sort_order: 4 },
      views: { is_pinned: true, sort_order: 2 },
    });
    expect(applySidebarPatches(previous, [{ key: "views" }])).toEqual({
      drafts: { is_pinned: true, sort_order: 4 },
      views: { is_pinned: true, sort_order: 0 },
    });
    expect(applySidebarPatches(undefined, [{ key: "drafts", is_pinned: false }])).toBeUndefined();
  });
});

describe("sidebar preference hooks", () => {
  function renderHandle(options: StubOptions = {}) {
    const { calls } = stubClient(options);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    let mutate: ((items: Array<{ key: string; is_pinned?: boolean; sort_order?: number }>) => void) | null = null;
    function Probe(): React.ReactElement {
      const query = useSidebarPreferences("acme");
      const mutation = useUpdateSidebarPreferences("acme");
      mutate = (items) => mutation.mutate(items);
      const entries = selectPersonalEntries(query.data);
      return <p>{`${query.status}|${entries.map((entry) => `${entry.key}:${entry.enabled}`).join(",")}`}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Probe />
      </QueryClientProvider>
    );
    return {
      calls,
      text: () => screen.getByText(/\|/).textContent ?? "",
      save: (items: Array<{ key: string; is_pinned?: boolean; sort_order?: number }>) => mutate?.(items),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("reads pins through the selector", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("success|drafts:true"));
  });

  it("toggles a pin optimistically and persists the triple", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("success|drafts:true"));
    h.save([{ key: "drafts", is_pinned: false }]);
    await waitFor(() => expect(h.text()).toBe("success|drafts:false"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain("/api/workspaces/acme/sidebar-preferences/");
    expect(JSON.parse(patch?.body ?? "[]")).toEqual([{ key: "drafts", is_pinned: false }]);
  });

  it("reverts the optimistic write and toasts when the save fails", async () => {
    const h = renderHandle({ patchStatus: 500 });
    await waitFor(() => expect(h.text()).toBe("success|drafts:true"));
    h.save([{ key: "drafts", is_pinned: false }]);
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't save sidebar preferences", expect.any(String))
    );
    expect(h.text()).toBe("success|drafts:true");
  });

  it("still sends the write while the first read hangs", async () => {
    const h = renderHandle({ hangGet: true });
    await screen.findByText("pending|drafts:true");
    h.save([{ key: "drafts", is_pinned: false }]);
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(notifyError).not.toHaveBeenCalled();
  });
});
