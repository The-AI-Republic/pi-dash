// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list screen (parity/specs/views.md §9, VIEW-029-032/039).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { makeView } from "../testing.js";
import { WorkspaceViewsListScreen } from "./WorkspaceViewsListScreen.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const DEFAULTS = ["All work items", "Assigned", "Created", "Subscribed"];
const VIEW_A = "623e4567-e89b-12d3-a456-426614174005";
const VIEW_B = "723e4567-e89b-12d3-a456-426614174006";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface Harness {
  views: ReturnType<typeof makeView>[];
  viewsStatus: number;
  viewsDelayMs: number;
  viewsCalls: number;
  workspaceName: string;
  workspaces: Record<string, unknown>[];
}

let harness: Harness;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  harness = {
    views: [],
    viewsStatus: 200,
    viewsDelayMs: 0,
    viewsCalls: 0,
    workspaceName: "Acme",
    workspaces: [{ id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 20 }],
  };
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.includes("/api/workspaces/acme/views/?")) {
      harness.viewsCalls += 1;
      if (harness.viewsDelayMs > 0) await sleep(harness.viewsDelayMs);
      return jsonResponse(url, harness.viewsStatus, harness.views);
    }
    if (url.endsWith("/api/users/me/workspaces/")) return jsonResponse(url, 200, harness.workspaces);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
  vi.restoreAllMocks();
});

function renderScreen(props: Partial<React.ComponentProps<typeof WorkspaceViewsListScreen>> = {}) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <WorkspaceViewsListScreen workspaceSlug="acme" {...props} />
    </QueryClientProvider>
  );
}

function defaultLabels(): string[] {
  return screen.queryAllByTestId("workspace-views-default-row").map((row) => row.textContent ?? "");
}

describe("WorkspaceViewsListScreen", () => {
  it("renders the search, the four defaults and the customs with the workspace tab title", async () => {
    harness.views = [
      makeView({ id: VIEW_A, name: "Alpha board", created_at: "2026-01-01T00:00:00Z" }),
      makeView({ id: VIEW_B, name: "Beta board", created_at: "2026-02-01T00:00:00Z" }),
    ];
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    expect(screen.getByTestId("workspace-views-search")).toBeInTheDocument();
    expect(defaultLabels()).toEqual(DEFAULTS);
    expect(screen.getAllByTestId("workspace-views-custom-row").map((row) => row.getAttribute("data-view-id"))).toEqual([
      VIEW_B,
      VIEW_A,
    ]);
    await waitFor(() => expect(document.title).toBe("Acme - All Views"));
  });

  it("shows the skeleton while customs load and never alongside the rows", async () => {
    harness.views = [makeView({ id: VIEW_A, name: "Alpha board" })];
    harness.viewsDelayMs = 60;
    const { container } = renderScreen();
    expect(screen.getByTestId("project-views-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-views-customs")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
    expect(container.querySelector('[data-skeleton-row="true"]')).toBeNull();
  });

  it("lists only the four defaults when the workspace has no customs", async () => {
    renderScreen();
    await waitFor(() => expect(document.title).toBe("Acme - All Views"));
    expect(await screen.findByTestId("workspace-views-defaults")).toBeInTheDocument();
    expect(defaultLabels()).toEqual(DEFAULTS);
    expect(screen.queryByTestId("workspace-views-customs")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-views-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-views-customs-no-match")).not.toBeInTheDocument();
  });

  it("filters both groups by a case-insensitive name match and hides an empty group", async () => {
    harness.views = [makeView({ id: VIEW_A, name: "Alpha board" }), makeView({ id: VIEW_B, name: "Beta board" })];
    renderScreen();
    await screen.findByTestId("workspace-views-customs");

    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "BETA" } });
    expect(defaultLabels()).toEqual([]);
    expect(screen.queryByTestId("workspace-views-defaults")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("workspace-views-custom-row").map((row) => row.getAttribute("data-view-id"))).toEqual([
      VIEW_B,
    ]);

    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "assigned" } });
    expect(defaultLabels()).toEqual(["Assigned"]);
    expect(screen.queryByTestId("workspace-views-customs")).not.toBeInTheDocument();
    expect(screen.getByTestId("workspace-views-customs-no-match")).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "nothing here" } });
    expect(defaultLabels()).toEqual([]);
    expect(screen.getByTestId("workspace-views-customs-no-match")).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "" } });
    expect(defaultLabels()).toEqual(DEFAULTS);
    expect(screen.getAllByTestId("workspace-views-custom-row")).toHaveLength(2);
  });

  it("renders the inline error and retries the read", async () => {
    harness.viewsStatus = 500;
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("project-views-error")).toBeInTheDocument());
    expect(defaultLabels()).toEqual(DEFAULTS);
    const callsBefore = harness.viewsCalls;
    harness.viewsStatus = 200;
    harness.views = [makeView({ id: VIEW_A, name: "Alpha board" })];
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    expect(harness.viewsCalls).toBe(callsBefore + 1);
  });

  it("threads the edit and delete slots into the custom rows", async () => {
    harness.views = [makeView({ id: VIEW_A, name: "Alpha board" })];
    const onEditView = vi.fn();
    const onDeleteView = vi.fn();
    renderScreen({ onEditView, onDeleteView });
    await screen.findByTestId("workspace-views-customs");
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha board" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit View" }));
    expect(onEditView).toHaveBeenCalledWith(expect.objectContaining({ id: VIEW_A }));
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha board" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete View" }));
    expect(onDeleteView).toHaveBeenCalledWith(expect.objectContaining({ id: VIEW_A }));
  });

  it("keeps the surface name alone while the workspace is unknown", async () => {
    harness.workspaces = [];
    renderScreen();
    await waitFor(() => expect(document.title).toBe("All Views"));
  });

  it("reads the customs once on mount", async () => {
    renderScreen();
    await waitFor(() => expect(harness.viewsCalls).toBe(1));
    await sleep(20);
    expect(harness.viewsCalls).toBe(1);
  });
});
