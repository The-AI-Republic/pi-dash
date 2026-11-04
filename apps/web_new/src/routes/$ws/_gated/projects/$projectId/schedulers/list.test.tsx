// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers list route composition (parity/specs/agents.md §2–§3,
// AGT-007/012): the route hands the list no creation trigger (the section
// tab row owns "New Scheduler", AGT-010), navigates a row to its install
// detail, and opens the edit and uninstall dialogs from the row actions
// without navigating.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { navigate, hooks } = vi.hoisted(() => ({
  navigate: vi.fn(),
  hooks: {
    bindings: { data: [] as SchedulerBinding[] | undefined, refetch: vi.fn() },
  },
}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => navigate };
});
vi.mock("../../../../../../features/schedulers/installData.js", () => ({
  useSchedulerBindings: () => hooks.bindings,
}));
vi.mock("../../../../../../features/schedulers/InstallsList.js", () => ({
  InstallsList: ({
    onCreate,
    onOpenInstall,
    onEditInstall,
    onUninstallInstall,
  }: {
    onCreate?: (() => void) | undefined;
    onOpenInstall: (binding: SchedulerBinding) => void;
    onEditInstall: (binding: SchedulerBinding) => void;
    onUninstallInstall: (binding: SchedulerBinding) => void;
  }) => (
    <div>
      {onCreate === undefined ? null : (
        <button type="button" onClick={onCreate}>
          New Scheduler
        </button>
      )}
      <button type="button" onClick={() => onOpenInstall(binding())}>
        Open install
      </button>
      <button type="button" onClick={() => onEditInstall(binding())}>
        Edit install
      </button>
      <button type="button" onClick={() => onUninstallInstall(binding())}>
        Uninstall install
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../features/schedulers/EditInstallDialog.js", () => ({
  EditInstallDialog: ({ binding, onSaved }: { binding: SchedulerBinding | null; onSaved: () => void }) => (
    <div data-testid="edit-dialog" data-binding={binding?.id ?? ""}>
      <button type="button" onClick={onSaved}>
        Settle after edit
      </button>
    </div>
  ),
}));
vi.mock("../../../../../../features/schedulers/UninstallInstallDialog.js", () => ({
  UninstallInstallDialog: ({ binding }: { binding: SchedulerBinding | null }) => (
    <div data-testid="uninstall-dialog" data-binding={binding?.id ?? ""} />
  ),
}));
import { Route } from "./list.js";

const PARAMS = { ws: "acme", projectId: "p1" };

function binding(): SchedulerBinding {
  return { id: "b1", scheduler: "s9", scheduler_slug: "weekly" } as SchedulerBinding;
}

afterEach(() => {
  vi.clearAllMocks();
  hooks.bindings = { data: [], refetch: vi.fn() };
});

function renderRoute(): void {
  vi.spyOn(Route, "useParams").mockReturnValue(PARAMS);
  const Component = Route.options.component;
  if (!Component) throw new Error("schedulers list route has no component");
  render(<Component />);
}

describe("schedulers list route", () => {
  it("hands the list no creation trigger: the section tab row owns New Scheduler", () => {
    hooks.bindings = { data: [binding()], refetch: vi.fn() };
    renderRoute();
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("install-dialog")).not.toBeInTheDocument();
  });

  it("opens and closes the edit dialog from a row, settling the list on save", async () => {
    const user = userEvent.setup();
    hooks.bindings = { data: [binding()], refetch: vi.fn() };
    renderRoute();
    expect(screen.getByTestId("edit-dialog")).toHaveAttribute("data-binding", "");
    await user.click(screen.getByRole("button", { name: "Edit install" }));
    expect(screen.getByTestId("edit-dialog")).toHaveAttribute("data-binding", "b1");
    await user.click(screen.getByRole("button", { name: "Settle after edit" }));
    expect(hooks.bindings.refetch).toHaveBeenCalled();
  });

  it("opens the uninstall dialog for the picked install only", async () => {
    const user = userEvent.setup();
    hooks.bindings = { data: [binding()], refetch: vi.fn() };
    renderRoute();
    expect(screen.getByTestId("uninstall-dialog")).toHaveAttribute("data-binding", "");
    await user.click(screen.getByRole("button", { name: "Uninstall install" }));
    expect(screen.getByTestId("uninstall-dialog")).toHaveAttribute("data-binding", "b1");
    expect(navigate).not.toHaveBeenCalled();
  });

  it("navigates a row to the install detail address", async () => {
    const user = userEvent.setup();
    renderRoute();
    await user.click(screen.getByRole("button", { name: "Open install" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/projects/p1/schedulers/b1" });
  });
});
