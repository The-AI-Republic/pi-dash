// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General leaf (PSET-001/PSET-004): the landing tab owns the slash-form
// address, draws the shell around the identity body and never refuses a
// role, so its page gate is its own screen rather than a denial.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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
import { Route } from "./index.js";

const GENERAL_ADDRESS = `/acme/settings/projects/${harness.params.projectId}/`;

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(harness.params);
  const Component = Route.options.component;
  if (!Component) throw new Error("general route has no component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {renderInRouter(GENERAL_ADDRESS, <Component />)}
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.role = 20;
  harness.workspaceRole = 20;
  vi.restoreAllMocks();
});

describe("general settings route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the identity body under the shell at the general address", async () => {
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { level: 1, name: "General" })).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-general")).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-general")).toHaveAttribute("data-active", "true");
  });

  it("keeps the page open for a guest while the rail narrows to general and members", async () => {
    harness.role = 5;
    harness.workspaceRole = 5;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    expect(screen.queryByTestId("project-settings-denied")).not.toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-members")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-tab-states")).not.toBeInTheDocument();
  });
});
