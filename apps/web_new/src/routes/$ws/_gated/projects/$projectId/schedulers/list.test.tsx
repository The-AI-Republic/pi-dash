// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers list route composition (parity/specs/agents.md §2,
// AGT-007): the route reads the caller's project role and workspace
// standing, hands the install dialog its permission flags and the list of
// already-installed definitions, opens the dialog from the list header and
// navigates a row to its install detail.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { navigate, hooks } = vi.hoisted(() => ({
  navigate: vi.fn(),
  hooks: {
    permissions: { isAdmin: false },
    roles: { isSuccess: true, isPending: false, data: {} as Record<string, number> },
    membership: { isSuccess: true, isPending: false },
    bindings: { data: [] as SchedulerBinding[], refetch: vi.fn() },
  },
}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => navigate };
});
vi.mock("../../../../../../core/session/index.js", () => ({
  usePermissions: () => hooks.permissions,
}));
vi.mock("../../../../../../features/auth/index.js", () => ({
  useProjectRoles: () => hooks.roles,
  useWorkspaceMembership: () => hooks.membership,
}));
vi.mock("../../../../../../features/schedulers/index.js", () => ({
  useSchedulerBindings: () => hooks.bindings,
  InstallsList: ({
    onCreate,
    onOpenInstall,
  }: {
    onCreate: () => void;
    onOpenInstall: (binding: SchedulerBinding) => void;
  }) => (
    <div>
      <button type="button" onClick={onCreate}>
        New Scheduler
      </button>
      <button type="button" onClick={() => onOpenInstall(binding())}>
        Open install
      </button>
    </div>
  ),
  InstallDialog: ({
    open,
    canInstall,
    canAuthor,
    installedSchedulerIds,
  }: {
    open: boolean;
    canInstall: boolean;
    canAuthor: boolean;
    installedSchedulerIds: string[];
  }) => (
    <div
      data-testid="install-dialog"
      data-open={open}
      data-can-install={canInstall}
      data-can-author={canAuthor}
      data-installed={installedSchedulerIds.join(",")}
    />
  ),
}));

import { Route } from "./list.js";

const PARAMS = { ws: "acme", projectId: "p1" };

function binding(): SchedulerBinding {
  return { id: "b1", scheduler: "s9", scheduler_slug: "weekly" } as SchedulerBinding;
}

afterEach(() => {
  vi.clearAllMocks();
  hooks.permissions = { isAdmin: false };
  hooks.roles = { isSuccess: true, isPending: false, data: {} };
  hooks.membership = { isSuccess: true, isPending: false };
  hooks.bindings = { data: [], refetch: vi.fn() };
});

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
  const Component = Route.options.component;
  if (!Component) throw new Error("schedulers list route has no component");
  render(<Component />);
}

describe("schedulers list route", () => {
  it("enables management for a project admin in a joined workspace", async () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    hooks.permissions = { isAdmin: true };
    hooks.bindings = { data: [binding()], refetch: vi.fn() };
    const user = userEvent.setup();
    renderRoute();
    const dialog = screen.getByTestId("install-dialog");
    expect(dialog).toHaveAttribute("data-can-install", "true");
    expect(dialog).toHaveAttribute("data-can-author", "true");
    expect(dialog).toHaveAttribute("data-installed", "s9");

    await user.click(screen.getByRole("button", { name: "New Scheduler" }));
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-open", "true");
  });

  it("keeps a plain member read-only and non-authoring", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 15 } };
    renderRoute();
    const dialog = screen.getByTestId("install-dialog");
    expect(dialog).toHaveAttribute("data-can-install", "false");
    expect(dialog).toHaveAttribute("data-can-author", "false");
  });

  it("refuses management while the standing reads are missing or failed", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    hooks.membership = { isSuccess: false, isPending: false };
    renderRoute();
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-can-install", "false");
  });

  it("gives a workspace admin authoring rights without project management", () => {
    hooks.permissions = { isAdmin: true };
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 15 } };
    renderRoute();
    const dialog = screen.getByTestId("install-dialog");
    expect(dialog).toHaveAttribute("data-can-install", "false");
    expect(dialog).toHaveAttribute("data-can-author", "true");
  });

  it("navigates a row to the install detail address", async () => {
    const user = userEvent.setup();
    renderRoute();
    await user.click(screen.getByRole("button", { name: "Open install" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/projects/p1/schedulers/b1" });
  });
});
