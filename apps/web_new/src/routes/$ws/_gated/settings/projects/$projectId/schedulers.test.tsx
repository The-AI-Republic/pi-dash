// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings schedulers (parity/specs/agents.md §2, AGT-007): the
// route mounts the settings shell around the install panel, refuses a
// non-admin with the oracle's denial heading while keeping the rail, and
// mounts the settings variant plus dialog for an admin.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: { ws: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001" },
  navigate: vi.fn(),
  role: 20,
  workspaceRole: 20,
  bindings: { data: [] as SchedulerBinding[], refetch: vi.fn() },
}));

vi.mock("../../../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => false,
}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => harness.navigate };
});

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
  useWorkspaceMembership: () => ({
    status: "success",
    data: { role: harness.workspaceRole },
    isPending: false,
    isSuccess: true,
  }),
  useJoinProject: () => ({ isPending: false, mutate: vi.fn() }),
}));

vi.mock("../../../../../../features/schedulers/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useSchedulerBindings: () => harness.bindings,
  InstallsList: ({
    variant,
    onCreate,
    onOpenInstall,
  }: {
    variant: string;
    onCreate: () => void;
    onOpenInstall: (binding: SchedulerBinding) => void;
  }) => (
    <div data-testid="installs-list" data-variant={variant}>
      <button type="button" onClick={onCreate}>
        New Scheduler
      </button>
      <button type="button" onClick={() => onOpenInstall({ id: "b1" } as SchedulerBinding)}>
        Open install
      </button>
    </div>
  ),
  InstallDialog: ({
    open,
    canInstall,
    canAuthor,
    installedSchedulerIds,
    onInstalled,
  }: {
    open: boolean;
    canInstall: boolean;
    canAuthor: boolean;
    installedSchedulerIds: string[];
    onInstalled: () => void;
  }) => (
    <div
      data-testid="install-dialog"
      data-open={open}
      data-can-install={canInstall}
      data-can-author={canAuthor}
      data-installed={installedSchedulerIds.join(",")}
    >
      <button type="button" onClick={onInstalled}>
        Settle list
      </button>
    </div>
  ),
}));

import { renderInRouter } from "../../../../../../features/project-settings/testing.js";
import { Route } from "./schedulers.js";

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(harness.params);
  const Component = Route.options.component;
  if (!Component) throw new Error("schedulers route has no component");
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {renderInRouter(`/acme/settings/projects/${harness.params.projectId}/schedulers/`, <Component />)}
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.role = 20;
  harness.workspaceRole = 20;
  harness.bindings = { data: [], refetch: vi.fn() };
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe("schedulers settings route", () => {
  it("carries no loader prefetch", () => {
    expect(Route.options.beforeLoad).toBeUndefined();
  });

  it("refuses a project member with the oracle heading, keeping the rail", async () => {
    harness.role = 15;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Oops! You are not authorized to view this page" })).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-schedulers")).toHaveAttribute("data-active", "true");
    expect(screen.queryByTestId("installs-list")).not.toBeInTheDocument();
  });

  it("refuses a workspace member without a project role", async () => {
    harness.role = null as unknown as number;
    harness.workspaceRole = 15;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: "Oops! You are not authorized to view this page" })).toBeInTheDocument();
  });

  it("mounts the settings variant and the dialog for a project admin", async () => {
    harness.workspaceRole = 15;
    harness.bindings = { data: [{ id: "b1", scheduler: "s9" } as SchedulerBinding], refetch: vi.fn() };
    const user = userEvent.setup();
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByRole("heading", { level: 1, name: "Schedulers" })).toBeInTheDocument();
    expect(screen.getByTestId("installs-list")).toHaveAttribute("data-variant", "settings");
    const dialog = screen.getByTestId("install-dialog");
    expect(dialog).toHaveAttribute("data-can-install", "true");
    expect(dialog).toHaveAttribute("data-can-author", "false");
    expect(dialog).toHaveAttribute("data-installed", "s9");

    await user.click(screen.getByRole("button", { name: "New Scheduler" }));
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-open", "true");

    await user.click(screen.getByRole("button", { name: "Open install" }));
    expect(harness.navigate).toHaveBeenCalledWith({
      href: `/acme/projects/${harness.params.projectId}/schedulers/b1`,
    });

    await user.click(screen.getByRole("button", { name: "Settle list" }));
    expect(harness.bindings.refetch).toHaveBeenCalled();
  });

  it("lets a workspace admin author definitions from the settings panel", async () => {
    harness.workspaceRole = 20;
    renderRoute();
    await waitFor(() => expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument());
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-can-author", "true");
  });
});
