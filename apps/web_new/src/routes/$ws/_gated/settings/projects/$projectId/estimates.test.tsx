// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates leaf (PSET-001/PSET-004): the route mounts the settings shell
// around the tab body, gates it for the role and keeps its own address.
// EST-024 pins the member/guest denial with no screen or controls.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, createRouter } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" },
  role: 20,
  workspaceRole: 20,
}));

vi.mock("../../../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => false,
}));

vi.mock("../../../../../../features/auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectRow: () => ({
    status: "success",
    data: { id: harness.params.projectId, name: "Website relaunch", identifier: "WEB", logo_props: {} },
    isPending: false,
    isSuccess: true,
  }),
  useProjectRoles: () => ({
    status: "success",
    data: { [harness.params.projectId]: harness.role },
    isPending: false,
    isSuccess: true,
  }),
  useWorkspaceMembership: () => ({ status: "success", data: { role: harness.workspaceRole }, isPending: false }),
  useJoinProject: () => ({ isPending: false, mutate: vi.fn() }),
}));

import { renderInRouter } from "../../../../../../features/project-settings/testing.js";
import { routeTree } from "../../../../../../routeTree.gen.js";
import { Route } from "./estimates.js";

vi.mock("../../../../../../features/estimates/EstimatesScreen.js", () => ({
  EstimatesScreen: () => <section aria-label="Estimates settings" data-testid="estimates-screen-stub" />,
}));

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(harness.params);
  const Component = Route.options.component;
  if (!Component) throw new Error("estimates route has no component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {renderInRouter(`/acme/settings/projects/${harness.params.projectId}/estimates/`, <Component />)}
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.role = 20;
  harness.workspaceRole = 20;
  vi.restoreAllMocks();
});

describe("estimates settings route", () => {
  it("primes the shell and tab chunks without a data read", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("settings route has no chunk prefetch");
    const fetch = vi.spyOn(globalThis, "fetch");
    await beforeLoad({} as never);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("mounts the tab inside the shell for an admin", async () => {
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { level: 1, name: "Estimates" })).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-estimates")).toHaveAttribute("data-active", "true");
    expect(await screen.findByTestId("estimates-screen-stub")).toBeInTheDocument();
  });

  it("refuses a role the page does not admit, keeping the rail", async () => {
    harness.role = 5;
    harness.workspaceRole = 5;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument());
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.queryByTestId("estimates-screen-stub")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("refuses a project member with no screen, switch or row controls", async () => {
    harness.role = 15;
    harness.workspaceRole = 15;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument());
    expect(screen.queryByTestId("estimates-screen-stub")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("refuses a member with no estimate controls", async () => {
    harness.role = 15;
    harness.workspaceRole = 15;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument());
    expect(screen.queryByTestId("estimates-screen-stub")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /add estimate/i })).not.toBeInTheDocument();
    expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument();
  });

  it("refuses a guest with no estimate controls", async () => {
    harness.role = 5;
    harness.workspaceRole = 5;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument());
    expect(screen.queryByTestId("estimates-screen-stub")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /add estimate/i })).not.toBeInTheDocument();
    expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument();
  });

  it("registers no per-system address below the estimates tab", () => {
    // EST-026: a guessed system id matches no estimates route — the
    // layout renders the ordinary not-found page for it (pinned in the
    // workspace-settings layout suite), never the estimates screen.
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ["/"] }),
      context: { queryClient: new QueryClient(), apiClient: {} as never },
      trailingSlash: "preserve",
    });
    const guess = `/acme/settings/projects/${harness.params.projectId}/estimates/44444444-4444-4444-8444-444444444444`;
    const matched = router.matchRoutes(guess) ?? [];
    expect(matched.length).toBeGreaterThan(0);
    expect(matched.map((match) => match.routeId)).not.toContain("/$ws/_gated/settings/projects/$projectId/estimates");
  });
});
