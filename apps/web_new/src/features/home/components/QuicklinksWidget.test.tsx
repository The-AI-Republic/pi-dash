// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { QuickLink, QuickLinkList } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { quicklinksKeys } from "../api/quicklinks.js";
import { QuicklinksWidget } from "./QuicklinksWidget.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");

function link(index: number, title: string | null, url = `https://example.com/${index}`): QuickLink {
  return {
    id: `00000000-0000-4000-8000-0000000000${String(index).padStart(2, "0")}`,
    workspace: "22222222-2222-4222-8222-222222222222",
    owner: "33333333-3333-4333-8333-333333333333",
    title,
    url,
    metadata: {},
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: new Date().toISOString(),
  };
}

function bulk(count: number): QuickLinkList {
  return Array.from({ length: count }, (_, index) => link(index, `Link ${index}`));
}

function renderWidget(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <QuicklinksWidget workspaceSlug="acme" />
    </QueryClientProvider>
  );
}

function renderSeeded(rows: QuickLinkList): QueryClient {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 500,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(quicklinksKeys.list("acme"), rows);
  renderWidget(queryClient);
  return queryClient;
}

function rowTitles(): string[] {
  return screen.getAllByTestId("quicklink-title").map((node) => node.textContent ?? "");
}

function openRowMenu(name: string): void {
  fireEvent.click(screen.getByRole("button", { name: `Link actions for ${name}` }));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
});

describe("QuicklinksWidget", () => {
  it("renders rows with names, ages and new-tab anchors", () => {
    renderSeeded([link(0, "Team docs"), link(1, null, "https://example.com/plain")]);
    expect(document.getElementById("home-quicklinks")).toHaveAttribute("data-widget-title", "Quicklinks");
    expect(rowTitles()).toEqual(["Team docs", "https://example.com/plain"]);

    const anchors = screen.getAllByRole("link");
    expect(anchors).toHaveLength(2);
    expect(anchors[0]).toHaveAttribute("href", "https://example.com/0");
    expect(anchors[0]).toHaveAttribute("target", "_blank");
    expect(anchors[0]).toHaveAttribute("rel", "noreferrer");
    expect(within(anchors[0] as HTMLElement).getByText(/just now|ago/)).toBeInTheDocument();
  });

  it("shows skeleton tiles while the list resolves", () => {
    createAppClient("https://api.test", (() => new Promise<never>(() => {})) as never);
    renderWidget(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    expect(screen.getByTestId("quicklinks-skeleton")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("quicklink-title")).not.toBeInTheDocument();
  });

  it("shows guidance at zero links", () => {
    renderSeeded([]);
    expect(screen.getByTestId("quicklinks-empty")).toHaveTextContent("No saved links yet");
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("keeps the header with no retry when the list fetch fails", async () => {
    createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    renderWidget(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    await waitFor(() => expect(console.error).toHaveBeenCalled());
    expect(screen.getByRole("heading", { name: "Quicklinks" })).toBeInTheDocument();
    expect(screen.queryByTestId("quicklink-title")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("collapses long lists behind an expander", () => {
    renderSeeded(bulk(8));
    expect(rowTitles()).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(rowTitles()).toHaveLength(8);
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(rowTitles()).toHaveLength(5);
  });

  it("shows short lists without an expander", () => {
    renderSeeded(bulk(5));
    expect(rowTitles()).toHaveLength(5);
    expect(screen.queryByRole("button", { name: /show all|show less/i })).not.toBeInTheDocument();
  });

  it("adds a link through the dialog", async () => {
    const user = userEvent.setup();
    const current = bulk(1);
    const posted: unknown[] = [];
    createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
      const method = init?.method ?? "GET";
      if (method === "GET" && url.endsWith("/quick-links/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(current),
        };
      }
      if (method === "POST") {
        posted.push(JSON.parse((init?.body as string) ?? "{}"));
        const created = link(9, "Guide", "https://example.org");
        current.push(created);
        return {
          status: 201,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(created),
        };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
    renderWidget(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    await waitFor(() => expect(rowTitles()).toEqual(["Link 0"]));

    await user.click(screen.getByRole("button", { name: "Add link" }));
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("Address"), "example.org");
    await user.type(screen.getByLabelText("Name"), "Guide");
    await user.click(screen.getByRole("button", { name: "Save link" }));

    await waitFor(() => expect(rowTitles()).toContain("Guide"));
    expect(posted).toEqual([{ url: "https://example.org", title: "Guide" }]);
    expect(notifySuccess).toHaveBeenCalledWith("Link saved");
  });

  it("edits a link through its row menu", async () => {
    const user = userEvent.setup();
    renderSeeded([link(0, "Team docs")]);
    openRowMenu("Team docs");
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));

    const dialog = await screen.findByRole("dialog", { name: "Edit link" });
    expect(within(dialog).getByLabelText("Address")).toHaveValue("https://example.com/0");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("deletes a link through its row menu", async () => {
    const current = bulk(2);
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      const method = init?.method ?? "GET";
      if (method === "GET" && url.endsWith("/quick-links/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(current),
        };
      }
      if (method === "DELETE") {
        const id = url.split("/").filter(Boolean).pop() ?? "";
        const next = current.filter((entry) => entry.id !== id);
        current.splice(0, current.length, ...next);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
    renderWidget(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    await waitFor(() => expect(rowTitles()).toEqual(["Link 0", "Link 1"]));

    openRowMenu("Link 0");
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));

    await waitFor(() => expect(rowTitles()).toEqual(["Link 1"]));
    expect(notifySuccess).toHaveBeenCalledWith("Link deleted");
  });

  it("keeps the row with a failure notice when delete fails", async () => {
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      const method = init?.method ?? "GET";
      if (method === "GET" && url.endsWith("/quick-links/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify([link(0, "Team docs")]),
        };
      }
      return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
    renderWidget(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    await waitFor(() => expect(rowTitles()).toEqual(["Team docs"]));

    openRowMenu("Team docs");
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Link delete failed", expect.anything()));
    expect(rowTitles()).toEqual(["Team docs"]);
  });

  it("copies the address to the clipboard", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
    renderSeeded([link(0, "Team docs")]);

    openRowMenu("Team docs");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy address" }));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://example.com/0"));
    expect(notifySuccess).toHaveBeenCalledWith("Link address copied");
  });

  it("notices when the clipboard refuses the write", async () => {
    Object.defineProperty(globalThis.navigator, "clipboard", {
      value: { writeText: async () => Promise.reject(new Error("denied")) },
      configurable: true,
    });
    renderSeeded([link(0, "Team docs")]);

    openRowMenu("Team docs");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy address" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Link copy failed"));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("notices when no clipboard exists", async () => {
    Object.defineProperty(globalThis.navigator, "clipboard", { value: undefined, configurable: true });
    renderSeeded([link(0, "Team docs")]);

    openRowMenu("Team docs");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy address" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Link copy failed"));
  });
});
