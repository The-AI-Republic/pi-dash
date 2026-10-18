// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { IssuesLayoutRoot } from "./IssuesLayoutRoot.js";
import { LAYOUT_ORDER, type LayoutKind } from "./kinds.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: UUID2 };
const PREFS_KEY = issueKeys.prefs("acme", "project", UUID2);

function prefsPayload(displayFilters: Record<string, unknown>) {
  return {
    id: UUID,
    workspace: UUID,
    project: UUID2,
    user: UUID,
    filters: {},
    display_filters: displayFilters,
    display_properties: {},
    rich_filters: {},
    preferences: {},
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

interface StubOptions {
  holdPatch?: { release: () => void } | null;
}

function renderRoot(
  options: {
    stub?: StubOptions;
    seed?: Record<string, unknown> | null;
    offered?: readonly LayoutKind[];
    totalCount?: number;
    resultCount?: number;
    slots?: {
      headerActions?: React.ReactNode;
      displayControl?: React.ReactNode;
      filterControl?: React.ReactNode;
      filterRow?: React.ReactNode;
      analyticsControl?: React.ReactNode;
    };
  } = {}
) {
  let serverFilters: Record<string, unknown> = {
    layout: "kanban",
    group_by: null,
    order_by: "sort_order",
  };
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
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
      if (options.stub?.holdPatch) {
        await new Promise<void>((resolve) => (options.stub!.holdPatch!.release = resolve));
      }
      const sent = JSON.parse(init.body ?? "{}") as { display_filters?: Record<string, unknown> };
      if (sent.display_filters !== undefined) serverFilters = sent.display_filters;
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(prefsPayload(serverFilters)),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(prefsPayload(serverFilters)),
    };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seed !== null) {
    queryClient.setQueryData(PREFS_KEY, prefsPayload(options.seed ?? serverFilters));
  }
  const rendered: Array<{ layout: LayoutKind; orderBy: string }> = [];
  render(
    <QueryClientProvider client={queryClient}>
      <React.Suspense fallback={<p>suspended</p>}>
        <IssuesLayoutRoot
          workspaceSlug="acme"
          entity={ENTITY}
          title="Website"
          totalCount={options.totalCount ?? 3}
          resultCount={options.resultCount ?? 3}
          offered={options.offered ?? LAYOUT_ORDER}
          renderLayout={(layout, prefs) => {
            rendered.push({ layout, orderBy: prefs.orderBy });
            return <p>{`layout:${layout}`}</p>;
          }}
          emptyState={<p>Nothing here yet</p>}
          noMatchState={<p>No issues match</p>}
          headerActions={options.slots?.headerActions}
          displayControl={options.slots?.displayControl}
          filterControl={options.slots?.filterControl}
          filterRow={options.slots?.filterRow}
          analyticsControl={options.slots?.analyticsControl}
        />
      </React.Suspense>
    </QueryClientProvider>
  );
  const segment = (name: string) =>
    within(screen.getByTestId("layout-switcher-segmented")).getByRole("button", { name });
  return { calls, rendered, segment };
}

describe("IssuesLayoutRoot", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("renders the stored layout with its prefs through the header", async () => {
    const h = renderRoot();
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Website" })).toBeInTheDocument();
    expect(screen.getByText("3 issues")).toBeInTheDocument();
    expect(h.segment("Board layout")).toHaveAttribute("data-active", "true");
    expect(h.rendered[h.rendered.length - 1]).toEqual({ layout: "kanban", orderBy: "sort_order" });
    expect(screen.getByTestId("mobile-layout-trigger")).toHaveTextContent("Board");
  });

  it("reads the preferences from the entity route when nothing is cached", async () => {
    const h = renderRoot({ seed: null });
    expect(screen.getByText("suspended")).toBeInTheDocument();
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    const get = h.calls.find((call) => call.method === "GET" && call.url.includes("user-properties"));
    expect(get?.url).toBe(`https://api.test/api/workspaces/acme/projects/${UUID2}/user-properties/`);
  });

  it("falls back to the first offered layout when the stored one is not registered", async () => {
    const h = renderRoot({ offered: ["list"] });
    expect(await screen.findByText("layout:list")).toBeInTheDocument();
    expect(h.segment("List layout")).toHaveAttribute("data-active", "true");
    expect(within(screen.getByTestId("layout-switcher-segmented")).getAllByRole("button")).toHaveLength(1);
  });

  it("persists a header switch and re-renders the new layout", async () => {
    const h = renderRoot();
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    fireEvent.click(h.segment("Calendar layout"));
    await waitFor(() => expect(screen.getByText("layout:calendar")).toBeInTheDocument());
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${UUID2}/user-properties/`);
    expect(JSON.parse(patch?.body ?? "{}").display_filters.layout).toBe("calendar");
    expect(h.segment("Calendar layout")).toHaveAttribute("data-active", "true");
  });

  it("persists a mobile menu switch", async () => {
    const h = renderRoot({ seed: { layout: "list", group_by: null, order_by: "sort_order" } });
    expect(await screen.findByText("layout:list")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("mobile-layout-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Calendar" }));
    await waitFor(() => expect(screen.getByText("layout:calendar")).toBeInTheDocument());
    expect(JSON.parse(h.calls.find((call) => call.method === "PATCH")?.body ?? "{}").display_filters.layout).toBe(
      "calendar"
    );
  });

  it("floats the mutation spinner while a switch settles", async () => {
    const holdPatch: { release: () => void } = { release: () => {} };
    const h = renderRoot({ stub: { holdPatch } });
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    expect(screen.queryByTestId("issues-mutation-spinner")).not.toBeInTheDocument();
    fireEvent.click(h.segment("List layout"));
    await waitFor(() => expect(screen.getByTestId("issues-mutation-spinner")).toBeInTheDocument());
    holdPatch.release();
    await waitFor(() => expect(screen.queryByTestId("issues-mutation-spinner")).not.toBeInTheDocument());
    expect(screen.getByText("layout:list")).toBeInTheDocument();
  });

  it("mounts the filter control in both headers and the row beneath them", async () => {
    renderRoot({
      slots: {
        filterControl: <button type="button">Filters</button>,
        filterRow: <div data-testid="filter-row-slot">row</div>,
      },
    });
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Filters" })).toHaveLength(2);
    expect(screen.getByTestId("filter-row-slot")).toBeInTheDocument();
  });

  it("routes zero totals and empty matches to the empty states", async () => {
    renderRoot({ totalCount: 0, resultCount: 0 });
    expect(await screen.findByText("Nothing here yet")).toBeInTheDocument();
    expect(screen.queryByText("layout:kanban")).not.toBeInTheDocument();
  });

  it("routes filtered-to-zero totals to the no-match state", async () => {
    renderRoot({ totalCount: 3, resultCount: 0 });
    expect(await screen.findByText("No issues match")).toBeInTheDocument();
    expect(screen.queryByText("Nothing here yet")).not.toBeInTheDocument();
  });

  it("renders the calendar grid even with no issues", async () => {
    const h = renderRoot({
      seed: { layout: "calendar", group_by: null, order_by: "sort_order" },
      totalCount: 0,
      resultCount: 0,
    });
    expect(await screen.findByText("layout:calendar")).toBeInTheDocument();
    expect(h.rendered).toHaveLength(1);
  });

  it("hosts the header, display and analytics slots", async () => {
    renderRoot({
      slots: {
        headerActions: <button type="button">Filters</button>,
        displayControl: <button type="button">Display</button>,
        analyticsControl: <button type="button">Analytics</button>,
      },
    });
    expect(await screen.findByText("layout:kanban")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filters" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Display" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Analytics" })).toBeInTheDocument();
  });
});
