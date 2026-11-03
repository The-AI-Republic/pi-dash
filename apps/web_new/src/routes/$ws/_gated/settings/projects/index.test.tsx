// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare settings path (PSET-002): forwards to the first joined project in
// sidebar order, or guides a viewer with none into creation.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: { ws: "acme" },
  navigate: vi.fn(),
  projects: [] as Array<Record<string, unknown>>,
  createVisible: false,
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: unknown) => ({ options, useParams: () => harness.params }),
  Navigate: ({ to }: { to: string }) => {
    harness.navigate(to);
    return null;
  },
}));

vi.mock("../../route.js", () => ({
  Route: { useRouteContext: () => ({ apiClient: {} }) },
}));

vi.mock("../../../../../features/projects/components/CreateProjectDialog.js", () => ({
  CreateProjectDialog: ({ open }: { open: boolean }) => {
    harness.createVisible = open;
    return open ? <p>project create dialog</p> : null;
  },
}));

import { resetAppClient } from "../../../../../core/api/client.js";
import { projectKeys } from "../../../../../features/projects/index.js";
import { Route, selectForwardTarget } from "./index.js";

const KEY_ARGS = ["acme"] as const;

afterEach(() => {
  resetAppClient();
  harness.navigate.mockReset();
  harness.createVisible = false;
});

function renderLanding(projects: Array<Record<string, unknown>> | undefined): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (projects !== undefined) {
    // Seed both collection reads the route may consult.
    queryClient.setQueryData(projectKeys.details(KEY_ARGS[0]), projects);
    queryClient.setQueryData(projectKeys.list(KEY_ARGS[0]), projects);
  }
  harness.projects = projects ?? [];
  const Component = Route.options.component;
  if (!Component) throw new Error("bare settings route has no component");
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

describe("selectForwardTarget", () => {
  it("picks the first joined, unarchived project in sidebar order", () => {
    const rows = [
      { id: "p-later", member_role: 15, archived_at: null, sort_order: 20 },
      { id: "p-outer", member_role: null, archived_at: null, sort_order: 1 },
      { id: "p-first", member_role: 20, archived_at: null, sort_order: 10 },
      { id: "p-archived", member_role: 20, archived_at: "2024-01-01T00:00:00Z", sort_order: 5 },
    ];
    expect(selectForwardTarget(rows as never, "acme")).toBe("/acme/settings/projects/p-first");
  });

  it("has no target without a membership", () => {
    expect(selectForwardTarget([], "acme")).toBeNull();
    expect(
      selectForwardTarget([{ id: "p", member_role: null, archived_at: null, sort_order: null }] as never, "acme")
    ).toBeNull();
  });
});

describe("bare settings route", () => {
  it("forwards to the first joined project", () => {
    renderLanding([
      { id: "p-second", member_role: 15, archived_at: null, sort_order: 20 },
      { id: "p-first", member_role: 20, archived_at: null, sort_order: 10 },
    ]);
    expect(harness.navigate).toHaveBeenCalledWith("/acme/settings/projects/p-first");
    expect(screen.queryByTestId("project-settings-guidance")).not.toBeInTheDocument();
  });

  it("guides a member of nothing and opens creation", () => {
    renderLanding([{ id: "p-outer", member_role: null, archived_at: null, sort_order: 10 }]);
    expect(harness.navigate).not.toHaveBeenCalled();
    expect(screen.getByTestId("project-settings-guidance")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(harness.createVisible).toBe(true);
    expect(screen.getByText("project create dialog")).toBeInTheDocument();
  });

  it("renders only the landing slot while the collection loads", () => {
    renderLanding(undefined);
    expect(screen.getByTestId("project-settings-landing")).toBeEmptyDOMElement();
    expect(screen.queryByTestId("project-settings-guidance")).not.toBeInTheDocument();
  });
});
