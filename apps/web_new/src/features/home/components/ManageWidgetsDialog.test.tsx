// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePreferenceList, HomeWidgetKey } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { widgetPrefsKeys } from "../api/widgetPrefs.js";
import { ManageWidgetsDialog } from "./ManageWidgetsDialog.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function pref(key: HomeWidgetKey, sort_order: number, is_enabled: boolean): HomePreferenceList[number] {
  return { key, is_enabled, config: {}, sort_order };
}

function seed(): HomePreferenceList {
  return [pref("quick_links", 100, true), pref("recents", 99, true), pref("my_stickies", 101, true)];
}

function stubPreferences(
  current: HomePreferenceList,
  writes: Array<{ url: string; body: unknown }>,
  failPatch: boolean,
  getImpl?: (call: number) => unknown
) {
  let getCalls = 0;
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    if (method === "GET" && url.endsWith("/home-preferences/")) {
      getCalls += 1;
      const override = getImpl?.(getCalls);
      if (override !== undefined) return override as never;
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(current),
      };
    }
    if (method === "PATCH") {
      const body = JSON.parse((init?.body as string) ?? "{}") as { is_enabled?: boolean; sort_order?: number };
      writes.push({ url, body });
      if (failPatch) {
        return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
      }
      const key = url.split("/").filter(Boolean).pop() ?? "";
      const row = current.find((entry) => entry.key === key);
      if (row !== undefined) {
        if (body.is_enabled !== undefined) row.is_enabled = body.is_enabled;
        if (body.sort_order !== undefined) row.sort_order = body.sort_order;
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          JSON.stringify({ key, is_enabled: row?.is_enabled ?? true, sort_order: row?.sort_order ?? 0 }),
      };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
}

function renderDialog(): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ManageWidgetsDialog workspaceSlug="acme" open onOpenChange={() => {}} />
    </QueryClientProvider>
  );
  return client;
}

function switchNames(): string[] {
  return screen.getAllByRole("switch").map((node) => node.getAttribute("aria-label") ?? "");
}

function layoutRows(tops: number[]): void {
  const rows = screen.getAllByRole("listitem");
  rows.forEach((row, index) => {
    const top = tops[index] ?? 0;
    row.getBoundingClientRect = () =>
      ({ top, height: 40, bottom: top + 40, left: 0, right: 100, width: 100, x: 0, y: top }) as DOMRect;
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("ManageWidgetsDialog", () => {
  it("lists surfaced widgets in stack order, never the retired key", async () => {
    stubPreferences(seed(), [], false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    expect(screen.getByRole("switch", { name: "Quicklinks" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText(/stick/i)).not.toBeInTheDocument();
    expect(document.body.textContent?.toLowerCase()).not.toContain("stick");
  });

  it("toggles a widget off instantly and silently", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));

    fireEvent.click(screen.getByRole("switch", { name: "Recents" }));
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Recents" })).toHaveAttribute("aria-checked", "false")
    );
    expect(writes).toEqual([
      { url: "https://api.test/api/workspaces/acme/home-preferences/recents/", body: { is_enabled: false } },
    ]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls back a failed toggle with a failure notice", async () => {
    stubPreferences(seed(), [], true);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));

    fireEvent.click(screen.getByRole("switch", { name: "Recents" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Widget toggle failed", expect.anything()));
    expect(screen.getByRole("switch", { name: "Recents" })).toHaveAttribute("aria-checked", "true");
  });

  it("drags a widget below another and persists the move", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[1] as HTMLElement, { clientY: 74 });
    expect(switchNames()).toEqual(["Recents", "Quicklinks"]);
    fireEvent.mouseUp(window);

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Widget order saved"));
    expect(writes).toEqual([
      { url: "https://api.test/api/workspaces/acme/home-preferences/quick_links/", body: { sort_order: 98 } },
    ]);
    expect(switchNames()).toEqual(["Recents", "Quicklinks"]);
  });

  it("drags a widget above another onto the top", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Recents" }));
    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[0] as HTMLElement, { clientY: 4 });
    expect(switchNames()).toEqual(["Recents", "Quicklinks"]);
    fireEvent.mouseUp(window);

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Widget order saved"));
    expect(writes).toEqual([
      { url: "https://api.test/api/workspaces/acme/home-preferences/recents/", body: { sort_order: 102 } },
    ]);
  });

  it("drops back home without a write", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    const rows = screen.getAllByRole("listitem");
    // Enters the lower row above its middle: still the top slot.
    fireEvent.mouseMove(rows[1] as HTMLElement, { clientY: 45 });
    expect(switchNames()).toEqual(["Quicklinks", "Recents"]);
    fireEvent.mouseUp(window);

    await waitFor(() => expect(screen.getByRole("switch", { name: "Quicklinks" })).toBeInTheDocument());
    expect(writes).toEqual([]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("steers a drag across two rows before dropping home", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    // First move: below the lower row's middle, so Quicklinks drops to the bottom.
    fireEvent.mouseMove(screen.getAllByRole("listitem")[1] as HTMLElement, { clientY: 74 });
    expect(switchNames()).toEqual(["Recents", "Quicklinks"]);
    // Second move: back above the top row's middle, steering home again. The
    // order updater now starts from the live order instead of the query rows.
    fireEvent.mouseMove(screen.getAllByRole("listitem")[0] as HTMLElement, { clientY: 45 });
    expect(switchNames()).toEqual(["Quicklinks", "Recents"]);
    fireEvent.mouseUp(window);

    await waitFor(() => expect(screen.getByRole("switch", { name: "Quicklinks" })).toBeInTheDocument());
    expect(writes).toEqual([]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("drops a drag silently when the map unloads mid-drag", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    // The seed resolves once; the unload's refetch hangs, freezing the
    // pending window the drop lands in.
    stubPreferences(seed(), writes, false, (call) => (call > 1 ? new Promise<never>(() => {}) : undefined));
    const client = renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[1] as HTMLElement, { clientY: 74 });
    expect(switchNames()).toEqual(["Recents", "Quicklinks"]);

    // The map unloads before release and the dialog re-renders pending:
    // the skeleton assertion pins the re-render (a bare removeQueries
    // leaves the last render — and its drop closure — untouched, and the
    // stale drop plans a real reorder and writes). The drop then plans
    // against no rows and stays silent.
    act(() => {
      void client.resetQueries({ queryKey: widgetPrefsKeys.list("acme") });
    });
    await waitFor(() => expect(screen.queryByTestId("manage-widgets-skeleton")).not.toBeNull());
    fireEvent.mouseUp(window);

    // Flush the mutation queue: a stale drop would land its PATCH here.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(writes).toEqual([]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("ignores a press without a move and moves over the dragged row", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[0] as HTMLElement, { clientY: 20 });
    fireEvent.mouseUp(window);

    expect(writes).toEqual([]);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("ignores hovering without a drag", async () => {
    const writes: Array<{ url: string; body: unknown }> = [];
    stubPreferences(seed(), writes, false);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[1] as HTMLElement, { clientY: 74 });
    expect(switchNames()).toEqual(["Quicklinks", "Recents"]);
    expect(writes).toEqual([]);
  });

  it("rolls back a failed reorder with a failure notice", async () => {
    stubPreferences(seed(), [], true);
    renderDialog();
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    layoutRows([0, 40]);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Reorder Quicklinks" }));
    const rows = screen.getAllByRole("listitem");
    fireEvent.mouseMove(rows[1] as HTMLElement, { clientY: 74 });
    fireEvent.mouseUp(window);

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Widget reorder failed", expect.anything()));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("shows skeleton rows while the map resolves", () => {
    createAppClient("https://api.test", (() => new Promise<never>(() => {})) as never);
    renderDialog();
    expect(screen.getByTestId("manage-widgets-skeleton")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders no rows and no retry when the map is empty or fails", async () => {
    createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    renderDialog();
    await waitFor(() => expect(console.error).toHaveBeenCalled());
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("manage-widgets-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("renders no rows for an empty map", async () => {
    stubPreferences([], [], false);
    renderDialog();
    await waitFor(() => expect(screen.queryByTestId("manage-widgets-skeleton")).not.toBeInTheDocument());
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("closes from its close control", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    stubPreferences(seed(), [], false);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ManageWidgetsDialog workspaceSlug="acme" open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(switchNames()).toEqual(["Quicklinks", "Recents"]));
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false, expect.anything()));
  });
});
