// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake leaf (PSET-001/PSET-004/PSET-045): the route mounts the settings
// shell around the one-switch body, gates it for the role and keeps its
// own address.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" },
  role: 20,
  workspaceRole: 20,
}));

vi.mock("../../../../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => false,
}));

vi.mock("../../../../../../../features/auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectRow: () => ({
    status: "success",
    data: {
      id: harness.params.projectId,
      name: "Website relaunch",
      identifier: "WEB",
      logo_props: {},
      inbox_view: true,
    },
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

import { renderInRouter } from "../../../../../../../features/project-settings/testing.js";
import { Route } from "./intake.js";

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(harness.params);
  const Component = Route.options.component;
  if (!Component) throw new Error("intake route has no component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {renderInRouter(`/acme/settings/projects/${harness.params.projectId}/features/intake/`, <Component />)}
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.role = 20;
  harness.workspaceRole = 20;
  vi.restoreAllMocks();
});

describe("intake settings route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("mounts the switch inside the shell for an admin", async () => {
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { level: 1, name: "Intake" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Enable intake" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("project-feature-settings")).toHaveAttribute("data-feature", "intake");
    expect(screen.getByTestId("project-settings-tab-intake")).toHaveAttribute("data-active", "true");
  });

  it("refuses a role the page does not admit, keeping the rail", async () => {
    harness.role = 5;
    harness.workspaceRole = 5;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument());
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).toBeNull();
  });
});
