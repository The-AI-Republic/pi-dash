// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePreferenceList, Transport } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { widgetPrefsKeys } from "../api/widgetPrefs.js";
import { HomeWidgetStack, type ManageWidgetsControls } from "./HomeWidgetStack.js";

vi.mock("./RecentsWidget.js", () => ({
  RecentsWidget: ({ workspaceSlug }: { workspaceSlug: string }) => (
    <section data-testid="recents-widget" data-widget-title="Recents" data-slug={workspaceSlug} />
  ),
}));

function stubTransport(pending = false): Transport {
  return async (url: string) => {
    if (pending) return new Promise<never>(() => {});
    return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
  };
}

function testClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function pref(key: string, sort_order: number, is_enabled: boolean): HomePreferenceList[number] {
  return { key: key as "recents", is_enabled, config: {}, sort_order };
}

function renderStack(queryClient: QueryClient, slots?: Partial<React.ComponentProps<typeof HomeWidgetStack>>): void {
  render(
    <QueryClientProvider client={queryClient}>
      <HomeWidgetStack workspaceSlug="acme" projects={[]} {...slots} />
    </QueryClientProvider>
  );
}

function ManageDialogStub(controls: ManageWidgetsControls): React.ReactElement {
  return (
    <div data-testid="manage-dialog" data-open={String(controls.open)}>
      <button
        type="button"
        onClick={() => {
          controls.onOpenChange(false);
        }}
      >
        Close
      </button>
    </div>
  );
}

let consoleError: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  resetAppClient();
  consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("HomeWidgetStack", () => {
  it("pairs the home breadcrumb with the widget-settings shortcut", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("recents", 99, true)]);
    renderStack(queryClient, { manageWidgets: (controls) => <ManageDialogStub {...controls} /> });

    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Home");
    expect(screen.getByText("Home")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("manage-dialog")).toHaveAttribute("data-open", "false");
    fireEvent.click(screen.getByRole("button", { name: "Manage widgets" }));
    expect(screen.getByTestId("manage-dialog")).toHaveAttribute("data-open", "true");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.getByTestId("manage-dialog")).toHaveAttribute("data-open", "false");
  });

  it("tolerates a missing dialog without crashing", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("recents", 99, true)]);
    renderStack(queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Manage widgets" }));
    expect(screen.getByTestId("recents-widget")).toBeInTheDocument();
  });

  it("shows the shell skeleton while the map resolves", () => {
    createAppClient("https://api.test", stubTransport(true));
    renderStack(testClient());
    expect(screen.getByText("Home")).toBeInTheDocument();
    expect(screen.getByTestId("dashboard-skeleton")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("recents-widget")).not.toBeInTheDocument();
  });

  it("renders enabled sections in map order", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [
      pref("recents", 99, true),
      pref("quick_links", 100, true),
      pref("my_stickies", 101, true),
    ]);
    renderStack(queryClient, {
      quicklinks: <section data-testid="quicklinks-widget" data-widget-title="Quicklinks" />,
    });

    const links = screen.getByTestId("quicklinks-widget");
    const recents = screen.getByTestId("recents-widget");
    expect(links.compareDocumentPosition(recents) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(recents).toHaveAttribute("data-slug", "acme");
    expect(screen.queryByTestId("dashboard-skeleton")).not.toBeInTheDocument();
  });

  it("hides toggled-off sections", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [
      pref("recents", 99, false),
      pref("quick_links", 100, true),
    ]);
    renderStack(queryClient, {
      quicklinks: <section data-testid="quicklinks-widget" data-widget-title="Quicklinks" />,
      emptyState: <div data-testid="all-off" />,
    });
    expect(screen.queryByTestId("recents-widget")).not.toBeInTheDocument();
    expect(screen.getByTestId("quicklinks-widget")).toBeInTheDocument();
    expect(screen.queryByTestId("all-off")).not.toBeInTheDocument();
  });

  it("shows the all-off slot when no known section is visible", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [
      pref("recents", 99, false),
      pref("my_stickies", 101, true),
    ]);
    renderStack(queryClient, { emptyState: <div data-testid="all-off" /> });
    expect(screen.getByTestId("all-off")).toBeInTheDocument();
    expect(screen.queryByTestId("recents-widget")).not.toBeInTheDocument();
  });

  it("renders nothing without known sections or an empty slot", () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("recents", 99, false)]);
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <HomeWidgetStack workspaceSlug="acme" projects={[]} />
      </QueryClientProvider>
    );
    expect(container.querySelector("#home-widget-stack")).toBeInTheDocument();
    expect(screen.queryByTestId("recents-widget")).not.toBeInTheDocument();
  });

  it("keeps the skeleton up when the map fetch fails, with no retry", async () => {
    createAppClient("https://api.test", stubTransport());
    renderStack(testClient());
    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    expect(consoleError).toHaveBeenCalledWith("[web_new] home fetch failed: widget-prefs/acme", expect.anything());
    expect(screen.getByTestId("dashboard-skeleton")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });

  it("keeps the painted stack when a map refetch fails", async () => {
    createAppClient("https://api.test", stubTransport());
    const queryClient = testClient();
    queryClient.setQueryData(widgetPrefsKeys.list("acme"), [pref("recents", 99, true)]);
    renderStack(queryClient);
    expect(screen.getByTestId("recents-widget")).toBeInTheDocument();

    await queryClient.invalidateQueries({ queryKey: widgetPrefsKeys.list("acme") });
    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    expect(screen.getByTestId("recents-widget")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry|try again/i })).not.toBeInTheDocument();
  });
});
