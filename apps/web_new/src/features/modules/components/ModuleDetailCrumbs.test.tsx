// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail trail (MOD-029): project and Modules links, the switcher
// last-crumb over every project module, and the narrow back control.

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
import { MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import { ModuleDetailCrumbs } from "./ModuleDetailCrumbs.js";

const MODULE_B = "423e4567-e89b-12d3-a456-426614174003";

const MODULES = [
  { id: MODULES_MODULE_ID, project_id: MODULES_PROJECT_ID, name: "Alpha", status: "planned", is_favorite: false },
  { id: MODULE_B, project_id: MODULES_PROJECT_ID, name: "Beta", status: "planned", is_favorite: false },
];

function renderCrumbs() {
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
    return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(MODULES) };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const detailRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/modules/$moduleId",
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ModuleDetailCrumbs
          workspaceSlug="acme"
          projectId={MODULES_PROJECT_ID}
          projectName="Website"
          moduleId={MODULES_MODULE_ID}
          moduleName="Alpha"
        />
      </QueryClientProvider>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([detailRoute])]),
    history: createMemoryHistory({
      initialEntries: [`/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`],
    }),
    defaultPreload: false,
  });
  return render(<RouterProvider router={router} />);
}

afterEach(() => {
  resetAppClient();
});

describe("ModuleDetailCrumbs", () => {
  it("links the project, the modules list, and the narrow back control", async () => {
    renderCrumbs();
    const project = await screen.findByTestId("module-crumb-project");
    expect(project).toHaveTextContent("Website");
    expect(project.closest("a")).toHaveAttribute("href", `/acme/projects/${MODULES_PROJECT_ID}/issues`);
    const modules = screen.getByTestId("module-crumb-modules");
    expect(modules).toHaveTextContent("Modules");
    expect(modules.closest("a")).toHaveAttribute("href", `/acme/projects/${MODULES_PROJECT_ID}/modules`);
    const back = screen.getByTestId("module-mobile-back");
    expect(back).toHaveAttribute("aria-label", "Back to modules");
    expect(back.closest("a")).toHaveAttribute("href", `/acme/projects/${MODULES_PROJECT_ID}/modules`);
  });

  it("shows the switcher naming the current module", async () => {
    renderCrumbs();
    const trigger = await screen.findByTestId("module-switcher-trigger");
    expect(trigger).toHaveAttribute("aria-label", "Switch module, current Alpha");
    expect(trigger).toHaveTextContent("Alpha");
  });

  it("lists every project module and navigates to the pick", async () => {
    renderCrumbs();
    fireEvent.click(await screen.findByTestId("module-switcher-trigger"));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Beta" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "No module" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Beta" }));
    expect(mocks.navigate).toHaveBeenCalledWith({ to: `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULE_B}` });
  });

  it("stays put when re-picking the current module", async () => {
    renderCrumbs();
    fireEvent.click(await screen.findByTestId("module-switcher-trigger"));
    fireEvent.click(await screen.findByRole("option", { name: "Alpha" }));
    expect(mocks.navigate).not.toHaveBeenCalled();
  });
});
