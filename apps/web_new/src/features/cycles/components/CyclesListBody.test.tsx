// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list body: the hero slot leads, upcoming and completed groups
// carry their counts and collapse independently, search narrows live,
// narrowing to zero falls to the no-match view for the cause, and the
// peeked cycle renders the quick look over the list.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen } from "@testing-library/react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { resetCycleSearchStore } from "../api/searchStore.js";
import { CYCLES_PROJECT_ID, jsonResponse, makeCycleRow, makeProjectRow } from "../testing.js";
import { CyclesListBody, type CyclesListBodyProps } from "./CyclesListBody.js";

const CURRENT_ID = "323e4567-e89b-12d3-a456-426614174101";
const UPCOMING_ID = "323e4567-e89b-12d3-a456-426614174102";
const DRAFT_ID = "323e4567-e89b-12d3-a456-426614174103";
const DONE_ID = "323e4567-e89b-12d3-a456-426614174104";
const CREATOR_ID = "123e4567-e89b-12d3-a456-426614174010";

const LIST_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/`;
const PROJECT_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/`;
const MEMBERS_PATH = "/api/workspaces/acme/members/";

interface Harness {
  cycles: CycleRowData[];
  roles: Record<string, number>;
  members: unknown[];
}

let harness: Harness;

function upcomingRow(): CycleRowData {
  return makeCycleRow({
    id: UPCOMING_ID,
    name: "November push",
    status: "upcoming",
    total_issues: 2,
    completed_issues: 0,
    cancelled_issues: 0,
    created_by: CREATOR_ID,
  });
}

function seedCycles(): CycleRowData[] {
  return [
    makeCycleRow({ id: CURRENT_ID, name: "Current cycle", status: "current", created_by: CREATOR_ID }),
    upcomingRow(),
    makeCycleRow({
      id: DRAFT_ID,
      name: "Draft cycle",
      status: "draft",
      start_date: null,
      end_date: null,
      created_by: CREATOR_ID,
    }),
    makeCycleRow({ id: DONE_ID, name: "Done cycle", status: "completed", created_by: CREATOR_ID }),
  ];
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  harness = {
    cycles: seedCycles(),
    roles: { [CYCLES_PROJECT_ID]: 20 },
    members: [
      {
        id: "223e4567-e89b-12d3-a456-426614174021",
        member: {
          id: CREATOR_ID,
          first_name: "Ada",
          last_name: "Lovelace",
          avatar: "",
          avatar_url: null,
          display_name: "Ada Lovelace",
          is_bot: false,
        },
        role: 20,
        is_active: true,
      },
    ],
  };
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(LIST_PATH)) return jsonResponse(url, 200, harness.cycles);
    if (url.includes(LIST_PATH)) {
      const row = harness.cycles.find((cycle) => url.includes(`/cycles/${cycle.id}/`));
      if (row === undefined) return jsonResponse(url, 404, { detail: "missing" });
      return jsonResponse(url, 200, row);
    }
    if (url.endsWith(PROJECT_PATH)) {
      return jsonResponse(url, 200, makeProjectRow({ timezone: "UTC" }));
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith(MEMBERS_PATH)) return jsonResponse(url, 200, harness.members);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  resetCycleSearchStore();
});

function renderBody(props: Partial<CyclesListBodyProps> = {}): void {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const listRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles",
    component: () => (
      <CyclesListBody
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        peekCycleId={null}
        onPeekChange={() => undefined}
        {...props}
      />
    ),
  });
  const detailRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles/$cycleId",
    component: () => <p>detail</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([listRoute, detailRoute])]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles`] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

describe("CyclesListBody", () => {
  it("leads with the hero slot, then counted groups without the current cycle", async () => {
    renderBody({ hero: <div data-testid="cycles-hero">hero</div> });
    expect(await screen.findByText("November push")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-hero")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upcoming cycle 2" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Completed cycle 1" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText("November push")).toBeInTheDocument();
    expect(screen.getByText("Draft cycle")).toBeInTheDocument();
    expect(screen.queryByText("Current cycle")).not.toBeInTheDocument();
    expect(screen.queryByText("Done cycle")).not.toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: "Ada Lovelace" })).toHaveLength(2);
    expect(screen.getAllByTestId("cycle-favorite-marker")).toHaveLength(2);
  });

  it("collapses each group independently and keeps the counts", async () => {
    renderBody();
    await screen.findByText("November push");
    fireEvent.click(screen.getByRole("button", { name: "Upcoming cycle 2" }));
    expect(screen.queryByText("November push")).not.toBeInTheDocument();
    expect(screen.queryByText("Draft cycle")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upcoming cycle 2" })).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(screen.getByRole("button", { name: "Completed cycle 1" }));
    expect(screen.getByText("Done cycle")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Completed cycle 1" }));
    expect(screen.queryByText("Done cycle")).not.toBeInTheDocument();
  });

  it("narrows rows live as the query types", async () => {
    renderBody();
    await screen.findByText("November push");
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "november" } });
    expect(screen.getByText("November push")).toBeInTheDocument();
    expect(screen.queryByText("Draft cycle")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upcoming cycle 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Completed cycle 0" })).toBeInTheDocument();
  });

  it("falls to the search no-match view and clears back to the rows", async () => {
    renderBody();
    await screen.findByText("November push");
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "zzz-no-such-orbit" } });
    expect(await screen.findByRole("heading", { name: "No matching cycles" })).toBeInTheDocument();
    expect(screen.getByText("Remove the search criteria to see all cycles")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Upcoming cycle/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(screen.getByText("November push")).toBeInTheDocument();
  });

  it("falls to the filters no-match view when the slice C predicate hides all rows", async () => {
    const onClearFilters = vi.fn();
    renderBody({ cycleFilter: () => false, onClearFilters });
    expect(await screen.findByText("Remove the filters to see all cycles")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear all filters" }));
    expect(onClearFilters).toHaveBeenCalledTimes(1);
  });

  it("names search when both search and filters hide the rows", async () => {
    renderBody({ cycleFilter: (cycle) => cycle.status !== "completed" });
    await screen.findByText("November push");
    fireEvent.click(screen.getByTestId("cycles-search-toggle"));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "zzz-no-such-orbit" } });
    expect(screen.getByText("Remove the search criteria to see all cycles")).toBeInTheDocument();
  });

  it("applies the filter predicate alongside the query", async () => {
    renderBody({ cycleFilter: (cycle) => cycle.status !== "draft" });
    expect(await screen.findByText("November push")).toBeInTheDocument();
    expect(screen.queryByText("Draft cycle")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upcoming cycle 1" })).toBeInTheDocument();
  });

  it("opens the peek from the eye and closes it back to the list", async () => {
    const onPeekChange = vi.fn();
    renderBody({ onPeekChange });
    await screen.findByText("November push");
    fireEvent.click(screen.getByRole("button", { name: "Quick look at November push" }));
    expect(onPeekChange).toHaveBeenCalledWith(UPCOMING_ID);
  });

  it("toggles the peek shut from the eye of the peeked row", async () => {
    const onPeekChange = vi.fn();
    renderBody({ onPeekChange, peekCycleId: UPCOMING_ID });
    await screen.findByTestId("cycles-list-body");
    expect(await screen.findByTestId("cycle-preview")).toBeInTheDocument();
    expect(await screen.findByTestId("cycle-peek-body")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Quick look at November push" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("peeks an unknown id through the detail read behind the error view", async () => {
    renderBody({ peekCycleId: "00000000-0000-0000-0000-000000000000" });
    await screen.findByTestId("cycles-list-body");
    expect(await screen.findByTestId("cycle-preview")).toBeInTheDocument();
    expect(await screen.findByTestId("cycle-peek-error")).toBeInTheDocument();
  });

  it("renders the filter and header slots", async () => {
    renderBody({
      filterControl: <div data-testid="cycles-filters">filters</div>,
      headerActions: <div data-testid="cycles-actions">actions</div>,
    });
    await screen.findByTestId("cycles-list-body");
    expect(screen.getByTestId("cycles-filters")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-actions")).toBeInTheDocument();
  });

  it("hides the star for guests", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderBody();
    await screen.findByText("November push");
    expect(screen.queryByTestId("cycle-favorite-marker")).not.toBeInTheDocument();
  });

  it("renders zero-count headers with no rows and no no-match view", async () => {
    harness.cycles = [];
    renderBody();
    await screen.findByTestId("cycles-list-body");
    expect(screen.getByRole("button", { name: "Upcoming cycle 0" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Completed cycle 0" })).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-no-match")).not.toBeInTheDocument();
  });

  it("keeps both empty group headers when only the current cycle is loaded", async () => {
    harness.cycles = [seedCycles()[0]!];
    renderBody();
    expect(await screen.findByRole("button", { name: "Upcoming cycle 0" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Completed cycle 0" })).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-no-match")).not.toBeInTheDocument();
  });
});
