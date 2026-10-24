// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail trail: project and Cycles links, the switcher last-crumb with
// its lifecycle marker, and the narrow back control (CYC-033, CYC-034).

import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_PROJECT_ID } from "../testing.js";
import { CycleDetailCrumbs } from "./CycleDetailCrumbs.js";

const CYCLE_A = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE_B = "423e4567-e89b-12d3-a456-426614174003";

const CYCLES = [
  { id: CYCLE_A, project_id: CYCLES_PROJECT_ID, name: "Sprint 1", is_favorite: false, status: "CURRENT" },
  { id: CYCLE_B, project_id: CYCLES_PROJECT_ID, name: "Sprint 2", is_favorite: false, status: "COMPLETED" },
];

function renderCrumbs(narrowActions: React.ReactNode = null) {
  mocks.navigate.mockClear();
  const transport = async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(CYCLES) };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const detailRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles/$cycleId",
    component: () => (
      <QueryClientProvider client={queryClient}>
        <CycleDetailCrumbs
          workspaceSlug="acme"
          projectId={CYCLES_PROJECT_ID}
          projectName="Website"
          cycleId={CYCLE_A}
          cycleName="Sprint 1"
          cycleStatus="CURRENT"
          narrowActions={narrowActions}
        />
      </QueryClientProvider>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([detailRoute])]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLE_A}`] }),
    defaultPreload: false,
  });
  return render(<RouterProvider router={router} />);
}

afterEach(() => {
  resetAppClient();
});

describe("CycleDetailCrumbs", () => {
  it("links the project, the cycles list, and the narrow back control", async () => {
    renderCrumbs();
    const project = await screen.findByTestId("cycle-crumb-project");
    expect(project).toHaveTextContent("Website");
    expect(project.closest("a")).toHaveAttribute("href", `/acme/projects/${CYCLES_PROJECT_ID}/issues`);
    const cycles = screen.getByTestId("cycle-crumb-cycles");
    expect(cycles).toHaveTextContent("Cycles");
    expect(cycles.closest("a")).toHaveAttribute("href", `/acme/projects/${CYCLES_PROJECT_ID}/cycles`);
    const back = screen.getByTestId("cycle-mobile-back");
    expect(back).toHaveAttribute("aria-label", "Back to cycles");
    expect(back.closest("a")).toHaveAttribute("href", `/acme/projects/${CYCLES_PROJECT_ID}/cycles`);
  });

  it("shows the switcher with the lifecycle marker", async () => {
    renderCrumbs();
    const trigger = await screen.findByTestId("cycle-switcher-trigger");
    expect(trigger).toHaveAttribute("aria-label", "Switch cycle, current Sprint 1");
    expect(trigger).toHaveTextContent("Sprint 1");
    expect(trigger).toHaveTextContent("In progress");
  });

  it("lists every live cycle and navigates to the pick", async () => {
    renderCrumbs();
    fireEvent.click(await screen.findByTestId("cycle-switcher-trigger"));
    expect(await screen.findByRole("option", { name: "Sprint 1" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Sprint 2" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "No cycle" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Sprint 2" }));
    expect(mocks.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLE_B}` });
  });

  it("stays put when re-picking the current cycle", async () => {
    renderCrumbs();
    fireEvent.click(await screen.findByTestId("cycle-switcher-trigger"));
    fireEvent.click(await screen.findByRole("option", { name: "Sprint 1" }));
    expect(mocks.navigate).not.toHaveBeenCalled();
  });

  it("mounts narrow actions beside the back control", async () => {
    renderCrumbs(<button type="button" data-testid="narrow-action" />);
    expect(await screen.findByTestId("cycle-mobile-back")).toBeInTheDocument();
    expect(screen.getByTestId("narrow-action")).toBeInTheDocument();
  });
});
